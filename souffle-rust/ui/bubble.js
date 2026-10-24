'use strict';
/* Bulle : affichage de l'état, gestes (clic = pause, clic droit = annuler,
 * glisser = déplacer). Le moteur vit dans l'application ; la page ne fait que
 * montrer et transmettre. */

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const pill = document.getElementById('pill');
const label = document.getElementById('label');
const hint = document.getElementById('hint');
const timer = document.getElementById('timer');
const bars = [...document.querySelectorAll('.bars span')];

let lastState = 'idle';
let retryable = false;
let tick = null;
/* Chrono de parole : il s'arrête pendant la pause et reprend où il en était. */
let elapsedMs = 0;
let runStart = 0;

invoke('bubble_hello').then((info) => {
  if (!info.glass) document.documentElement.classList.add('no-glass');
}).catch(() => document.documentElement.classList.add('no-glass'));

/* ---------- Gestes ----------
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
  const dx = e.screenX - press.x;
  const dy = e.screenY - press.y;
  if (!press.dragging) {
    if (Math.abs(dx) + Math.abs(dy) < DRAG_THRESHOLD) return;
    press.dragging = true;
    pill.classList.add('dragging');
    invoke('bubble_drag_start');
  }
  invoke('bubble_drag_move', { dx, dy });
});

function release(e, cancelled) {
  if (!press || e.pointerId !== press.id) return;
  const wasDrag = press.dragging;
  press = null;
  pill.classList.remove('dragging');
  if (wasDrag) {
    invoke('bubble_drag_end');
    return;
  }
  if (cancelled) return;
  const state = pill.dataset.state;
  if (state === 'recording' || state === 'paused' || (state === 'error' && retryable)) invoke('bubble_click');
}
pill.addEventListener('pointerup', (e) => release(e, false));
pill.addEventListener('pointercancel', (e) => release(e, true));

pill.addEventListener('contextmenu', (e) => {
  e.preventDefault();
  const state = pill.dataset.state;
  if (state === 'recording' || state === 'paused') invoke('bubble_right_click');
});

/* ---------- Rendu ---------- */

const COPY = {
  idle: ['', ''],
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

function restBars() {
  bars.forEach((b) => (b.style.transform = 'scaleY(0.22)'));
  pill.style.setProperty('--lvl', 0);
}

function render(v) {
  const { state, message, step, words, pasted, live, segments, retry } = v;
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
    state === 'working' && step ? step.charAt(0).toUpperCase() + step.slice(1)
    : state === 'done' && words ? `${words} mot${words > 1 ? 's' : ''}`
    : state === 'error' && retryable && /réseau/.test(message || '') ? 'Audio gardé · nouvel essai tout seul'
    : state === 'error' && retryable ? 'Cliquez pour réessayer'
    : state === 'error' && /^Micro/.test(message || '') ? 'Réglages Système › Micro'
    : state === 'error' && /Accessibilité/.test(message || '') ? 'Réglages Système › Accessibilité'
    : state === 'error' ? 'Réglages de Souffle'
    : state === 'recording' && live && segments
      ? `${segments} phrase${segments > 1 ? 's' : ''} écrite${segments > 1 ? 's' : ''}`
      : h;
  pill.setAttribute('aria-label', `${label.textContent}. ${hint.textContent}`);

  // En mode direct, l'état « recording » est réémis à chaque phrase écrite :
  // le chrono ne repart qu'au vrai début de la dictée.
  const previous = lastState;
  lastState = state;
  if (state === previous) return;

  if (state === 'recording') {
    if (previous !== 'paused') elapsedMs = 0;
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
    restBars();
    return;
  }
  clearInterval(tick);
  tick = null;
  runStart = 0;
  restBars();
}

/* Chaque barre réagit avec un décalage : ça se lit comme une vraie onde. */
const WEIGHTS = [0.55, 0.85, 1, 0.85, 0.55];
let smooth = 0;

function renderLevel(level) {
  if (pill.dataset.state !== 'recording') return;
  smooth = smooth * 0.5 + level * 0.5;
  pill.style.setProperty('--lvl', smooth.toFixed(3));
  bars.forEach((bar, i) => {
    const s = 0.22 + smooth * WEIGHTS[i] * 0.78 + Math.random() * 0.06;
    bar.style.transform = `scaleY(${Math.min(1, s).toFixed(3)})`;
  });
}

listen('bubble:view', (e) => render(e.payload));
listen('bubble:level', (e) => renderLevel(e.payload));
