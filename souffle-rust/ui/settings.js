'use strict';
/* Réglages : chaque changement est enregistré aussitôt. */

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const $ = (id) => document.getElementById(id);
const $$ = (sel, root = document) => [...root.querySelectorAll(sel)];

let cfg = {};
const isMac = () => cfg.platform === 'macos';

const MODELS = {
  stt: {
    groq: ['whisper-large-v3-turbo', 'whisper-large-v3'],
    openai: ['gpt-4o-mini-transcribe', 'gpt-4o-transcribe', 'whisper-1'],
    custom: []
  },
  llm: {
    groq: ['openai/gpt-oss-120b', 'llama-3.3-70b-versatile', 'openai/gpt-oss-20b'],
    openai: ['gpt-4o-mini', 'gpt-4.1-mini', 'gpt-5-mini'],
    custom: []
  }
};
const HELP = {
  stt: {
    groq: 'Le plus rapide. Pour l’allemand ou l’anglais, whisper-large-v3 est plus précis que la version turbo.',
    openai: 'Le plus précis en français.',
    custom: 'Le nom du modèle tel que votre serveur l’attend.'
  },
  llm: {
    groq: 'openai/gpt-oss-120b comprend mieux vos reprises (« non, jeudi », « oublie ça ») et reste très rapide.',
    openai: 'gpt-4o-mini : rapide et sûr.',
    custom: 'Le nom du modèle tel que votre serveur l’attend.'
  }
};
const TONES = [['brut', 'Brut'], ['neutre', 'Neutre'], ['formel', 'Formel'], ['casual', 'Détendu']];

/* ---------- Petits outils ---------- */

let toastTimer = null;
function toast(message, bad = false) {
  const t = $('toast');
  t.textContent = message;
  t.classList.toggle('bad', bad);
  t.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (t.hidden = true), bad ? 5000 : 1800);
}

async function save(patch, quiet = true) {
  try {
    cfg = await invoke('set_settings', { patch });
    render();
    if (!quiet) toast('Enregistré');
    return true;
  } catch (err) {
    toast(String(err), true);
    render();
    return false;
  }
}

function debounce(fn, ms) {
  let t = null;
  return (...a) => {
    clearTimeout(t);
    t = setTimeout(() => fn(...a), ms);
  };
}

function applyTheme(theme) {
  if (theme === 'light' || theme === 'dark') document.documentElement.dataset.theme = theme;
  else delete document.documentElement.dataset.theme;
}

/* ---------- Onglets ---------- */

function showTab(name) {
  if (!$(`tab-${name}`)) name = 'general';
  $$('.tab').forEach((t) => (t.hidden = t.id !== `tab-${name}`));
  $$('.nav button').forEach((b) => b.classList.toggle('active', b.dataset.tab === name));
  $('main').scrollTop = 0;
  if (name === 'history') loadHistory();
  if (name === 'general') loadMicrophones();
}
$$('.nav button').forEach((b) => b.addEventListener('click', () => showTab(b.dataset.tab)));
document.addEventListener('click', (e) => {
  const goto = e.target.closest('[data-goto]');
  if (goto) {
    e.preventDefault();
    showTab(goto.dataset.goto);
  }
  const link = e.target.closest('[data-link]');
  if (link) {
    e.preventDefault();
    invoke('open_link', { url: link.dataset.link });
  }
});

/* ---------- Rendu ---------- */

function render() {
  applyTheme(cfg.theme);
  $('version').textContent = `version ${cfg.version}`;
  $('aboutVersion').textContent = cfg.version;

  $('shortcut').textContent = cfg.shortcutPretty || cfg.shortcut;
  $('shortcutError').hidden = !cfg.shortcutError;
  $('shortcutError').textContent = cfg.shortcutError || '';
  const anyKey = Object.values(cfg.secrets || {}).some(Boolean);
  $('imported').hidden = !(cfg.importedFromElectron && !anyKey);

  $('modeHelp').textContent = cfg.mode === 'hold'
    ? 'Gardez la touche enfoncée pendant que vous parlez. Un appui bref fonctionne comme la bascule.'
    : 'Un appui démarre, un second termine.';
  $('escapeHelp').textContent = isMac() && !cfg.accessibility
    ? 'Sur Mac, Échap demande l’autorisation d’Accessibilité.'
    : '';

  for (const seg of $$('.seg[data-key]')) {
    for (const b of $$('button', seg)) b.classList.toggle('on', String(cfg[seg.dataset.key]) === b.dataset.value);
  }
  for (const id of ['liveMode', 'playSounds', 'autoPaste', 'refocusTarget', 'restoreClipboard', 'launchAtLogin', 'providerFallback', 'formatEnabled', 'privacyMode']) {
    $(id).checked = Boolean(cfg[id]);
  }
  for (const id of ['maxMinutes', 'language', 'tone', 'targetLanguage']) $(id).value = String(cfg[id]);
  for (const id of ['sttModel', 'llmModel', 'customSttUrl', 'customLlmUrl', 'historyLimit']) {
    if (document.activeElement !== $(id)) $(id).value = cfg[id] ?? '';
  }
  if (document.activeElement !== $('customInstructions')) $('customInstructions').value = cfg.customInstructions || '';
  if (document.activeElement !== $('dictionary')) $('dictionary').value = (cfg.dictionary || []).join('\n');
  if ($('microphone').dataset.loaded) $('microphone').value = cfg.microphone || '';

  fillModels('stt', cfg.sttProvider);
  fillModels('llm', cfg.llmProvider);

  // Lignes conditionnelles : data-show="clé" ou "clé=valeur".
  for (const row of $$('[data-show]')) {
    const [k, v] = row.dataset.show.split('=');
    let show = v === undefined ? Boolean(cfg[k]) : String(cfg[k]) === v;
    if (k === 'llmProvider' || k === 'outputMode') show = show && cfg.formatEnabled;
    row.hidden = !show;
  }
  for (const el of $$('[data-only]')) el.hidden = cfg.platform !== el.dataset.only;
  for (const k of $$('kbd.mod')) k.textContent = isMac() ? '⌘' : 'Ctrl+';

  for (const [key, on] of Object.entries(cfg.secrets || {})) {
    const s = document.querySelector(`[data-status="${key}"]`);
    if (!s) continue;
    s.textContent = on ? 'enregistrée' : 'absente';
    s.className = `pill-status ${on ? 'on' : ''}`;
  }

  const n = cfg.waitingOnDisk || 0;
  $('waitingHelp').textContent = n
    ? `${n} dictée${n > 1 ? 's' : ''} attend${n > 1 ? 'ent' : ''} d’être retranscrite${n > 1 ? 's' : ''} (réseau ou fournisseur indisponible au moment de parler).`
    : 'Aucune : tout ce que vous avez dit a été transcrit.';
  $('retryPending').disabled = !n;

  const st = cfg.stats || { words: 0, sessions: 0, seconds: 0 };
  $('statWords').textContent = st.words.toLocaleString('fr-FR');
  $('statSessions').textContent = st.sessions.toLocaleString('fr-FR');
  $('statSaved').textContent = Math.max(0, Math.round(st.words / 45 - st.seconds / 60)).toLocaleString('fr-FR');

  const mic = cfg.microphonePermission;
  $('micStatus').textContent = mic === 'granted' ? 'accordé' : mic === 'denied' ? 'refusé' : 'à demander';
  $('micStatus').className = `pill-status ${mic === 'granted' ? 'on' : 'off'}`;
  $('micButton').hidden = mic === 'granted';
  $('micButton').textContent = mic === 'denied' ? 'Ouvrir les réglages' : 'Autoriser';
  $('axStatus').textContent = cfg.accessibility ? 'accordée' : 'à autoriser';
  $('axStatus').className = `pill-status ${cfg.accessibility ? 'on' : 'off'}`;
  $('axButton').hidden = Boolean(cfg.accessibility);

  renderPairs();
}

function fillModels(kind, provider) {
  const list = $(`${kind}Models`);
  const wanted = (MODELS[kind][provider] || []).join('|');
  if (list.dataset.for !== wanted) {
    list.innerHTML = '';
    for (const m of MODELS[kind][provider] || []) {
      const o = document.createElement('option');
      o.value = m;
      list.append(o);
    }
    list.dataset.for = wanted;
  }
  $(`${kind}Help`).textContent = HELP[kind][provider] || '';
}

/* ---------- Commandes simples ---------- */

for (const seg of $$('.seg[data-key]')) {
  seg.addEventListener('click', async (e) => {
    const b = e.target.closest('button');
    if (!b) return;
    const key = seg.dataset.key;
    const patch = { [key]: b.dataset.value };
    // Changer de fournisseur : on propose son modèle conseillé si l'actuel ne lui appartient pas.
    if (key === 'sttProvider' && b.dataset.value !== 'custom' && !MODELS.stt[b.dataset.value].includes(cfg.sttModel)) {
      patch.sttModel = MODELS.stt[b.dataset.value][0];
    }
    if (key === 'llmProvider' && b.dataset.value !== 'custom' && !MODELS.llm[b.dataset.value].includes(cfg.llmModel)) {
      patch.llmModel = MODELS.llm[b.dataset.value][0];
    }
    if (key === 'theme') applyTheme(b.dataset.value);
    await save(patch);
  });
}

for (const id of ['liveMode', 'playSounds', 'autoPaste', 'refocusTarget', 'restoreClipboard', 'launchAtLogin', 'providerFallback', 'formatEnabled', 'privacyMode']) {
  $(id).addEventListener('change', () => save({ [id]: $(id).checked }));
}
$('maxMinutes').addEventListener('change', () => save({ maxMinutes: parseInt($('maxMinutes').value, 10) || 10 }));
for (const id of ['language', 'tone', 'targetLanguage', 'microphone']) {
  $(id).addEventListener('change', () => save({ [id]: $(id).value }));
}
for (const id of ['sttModel', 'llmModel', 'customSttUrl', 'customLlmUrl']) {
  $(id).addEventListener('change', () => save({ [id]: $(id).value.trim() }));
}
$('historyLimit').addEventListener('change', () => {
  const v = Math.min(2000, Math.max(10, parseInt($('historyLimit').value, 10) || 100));
  save({ historyLimit: v });
});
$('customInstructions').addEventListener('input', debounce(() => save({ customInstructions: $('customInstructions').value }), 600));
$('dictionary').addEventListener('input', debounce(() => {
  const words = $('dictionary').value.split('\n').map((w) => w.trim()).filter(Boolean);
  save({ dictionary: words });
}, 600));

$('try').addEventListener('click', () => invoke('dictate'));
$('retryPending').addEventListener('click', () => {
  invoke('retry_pending');
  toast('Retranscription lancée');
});
for (const id of ['openData', 'openData2']) {
  $(id).addEventListener('click', (e) => {
    e.preventDefault();
    invoke('open_data_folder');
  });
}
$('axButton').addEventListener('click', () => invoke('open_accessibility'));
$('micButton').addEventListener('click', async () => {
  if (cfg.microphonePermission === 'denied') invoke('open_microphone_settings');
  else await invoke('request_microphone');
  reload();
});

/* ---------- Micro ---------- */

async function loadMicrophones() {
  const sel = $('microphone');
  try {
    const mics = await invoke('microphones');
    sel.innerHTML = '<option value="">Micro du système</option>';
    for (const m of mics) {
      const o = document.createElement('option');
      o.value = o.textContent = m;
      sel.append(o);
    }
    if (cfg.microphone && !mics.includes(cfg.microphone)) {
      const o = document.createElement('option');
      o.value = cfg.microphone;
      o.textContent = `${cfg.microphone} (débranché)`;
      sel.append(o);
    }
    sel.dataset.loaded = '1';
    sel.value = cfg.microphone || '';
  } catch {
    /* liste indisponible : le micro du système reste choisi */
  }
}

/* ---------- Raccourci ---------- */

const KEY_NAMES = {
  Space: 'Space', Enter: 'Enter', Tab: 'Tab', Backspace: 'Backspace', Delete: 'Delete',
  ArrowUp: 'Up', ArrowDown: 'Down', ArrowLeft: 'Left', ArrowRight: 'Right',
  Home: 'Home', End: 'End', PageUp: 'PageUp', PageDown: 'PageDown', Insert: 'Insert'
};

function accelFrom(e) {
  const code = e.code;
  if (['ShiftLeft', 'ShiftRight', 'ControlLeft', 'ControlRight', 'AltLeft', 'AltRight', 'MetaLeft', 'MetaRight'].includes(code)) return null;
  let key = KEY_NAMES[code];
  if (!key) {
    if (/^Key[A-Z]$/.test(code)) key = code.slice(3);
    else if (/^Digit\d$/.test(code)) key = code.slice(5);
    else if (/^F\d{1,2}$/.test(code)) key = code;
    else key = code; // Minus, Comma, Backquote… compris tels quels
  }
  const parts = [];
  if (e.ctrlKey) parts.push('Control');
  if (e.altKey) parts.push('Alt');
  if (e.shiftKey) parts.push('Shift');
  if (e.metaKey) parts.push(isMac() ? 'Command' : 'Super');
  parts.push(key);
  return parts.join('+');
}

let capturing = false;
$('shortcut').addEventListener('click', () => {
  capturing = !capturing;
  $('shortcut').classList.toggle('listening', capturing);
  $('shortcut').textContent = capturing ? 'Tapez la combinaison…' : cfg.shortcutPretty;
});
window.addEventListener('keydown', async (e) => {
  if (!capturing) return;
  e.preventDefault();
  if (e.code === 'Escape' && !e.ctrlKey && !e.altKey && !e.metaKey && !e.shiftKey) {
    capturing = false;
    $('shortcut').classList.remove('listening');
    $('shortcut').textContent = cfg.shortcutPretty;
    return;
  }
  const accel = accelFrom(e);
  if (!accel) return;
  try {
    await invoke('validate_shortcut', { accel });
  } catch (err) {
    toast(String(err), true);
    return;
  }
  capturing = false;
  $('shortcut').classList.remove('listening');
  if (await save({ shortcut: accel })) toast(`Nouveau raccourci : ${cfg.shortcutPretty}`);
});

/* ---------- Clés API ---------- */

for (const input of $$('[data-secret]')) {
  input.addEventListener('change', async () => {
    const value = input.value.trim();
    cfg.secrets = await invoke('set_secret', { key: input.dataset.secret, value });
    input.value = '';
    input.placeholder = value ? '•••••••• (enregistrée)' : input.placeholder;
    render();
    toast(value ? 'Clé enregistrée dans le trousseau' : 'Clé supprimée');
  });
}
for (const b of $$('[data-test]')) {
  b.addEventListener('click', async () => {
    const provider = b.dataset.test;
    const input = document.querySelector(`.key[data-provider="${provider}"] input`);
    const out = document.querySelector(`[data-result="${provider}"]`);
    out.className = 'key-result';
    out.textContent = 'Test en cours…';
    b.disabled = true;
    const customUrl = cfg.customLlmUrl || cfg.customSttUrl || '';
    try {
      const r = await invoke('test_key', { provider, key: input.value, customUrl });
      out.className = `key-result ${r.ok ? 'ok' : 'bad'}`;
      out.textContent = r.ok ? `La clé fonctionne : ${r.models} modèles disponibles.` : r.error;
    } catch (err) {
      out.className = 'key-result bad';
      out.textContent = String(err);
    }
    b.disabled = false;
  });
}

/* ---------- Raccourcis de texte et tons par application ---------- */

function pairRow(first, second, onChange, onRemove, secondIsTone) {
  const row = document.createElement('div');
  row.className = 'pair';
  const a = document.createElement('input');
  a.value = first;
  a.placeholder = secondIsTone ? 'Application' : 'Ce que vous dites';
  let b;
  if (secondIsTone) {
    b = document.createElement('select');
    for (const [v, l] of TONES) b.append(new Option(l, v));
    b.value = second || 'neutre';
  } else {
    b = document.createElement('input');
    b.value = second;
    b.placeholder = 'Ce que Souffle écrit';
  }
  const x = document.createElement('button');
  x.className = 'x';
  x.textContent = '×';
  x.title = 'Retirer';
  a.addEventListener('change', () => onChange(a.value.trim(), b.value));
  b.addEventListener('change', () => onChange(a.value.trim(), b.value));
  x.addEventListener('click', onRemove);
  row.append(a, b, x);
  return row;
}

let draftSnippet = false;
let draftTone = false;

function renderPairs() {
  if (document.activeElement && document.activeElement.closest('.pairs')) return;
  const sn = $('snippets');
  sn.innerHTML = '';
  const snippets = cfg.snippets || [];
  snippets.forEach((s, i) => {
    sn.append(pairRow(s.trigger, s.value, (t, v) => {
      const next = snippets.slice();
      next[i] = { trigger: t, value: v };
      save({ snippets: next.filter((x) => x.trigger || x.value) });
    }, () => save({ snippets: snippets.filter((_, j) => j !== i) })));
  });
  if (draftSnippet) {
    sn.append(pairRow('', '', (t, v) => {
      if (!t || !v) return;
      draftSnippet = false;
      save({ snippets: [...snippets, { trigger: t, value: v }] });
    }, () => { draftSnippet = false; renderPairs(); }));
  }

  const at = $('appTones');
  at.innerHTML = '';
  const tones = cfg.appTones || {};
  for (const [app, tone] of Object.entries(tones)) {
    at.append(pairRow(app, tone, (a, t) => {
      const next = { ...tones };
      delete next[app];
      if (a) next[a] = t;
      save({ appTones: next });
    }, () => {
      const next = { ...tones };
      delete next[app];
      save({ appTones: next });
    }, true));
  }
  if (draftTone) {
    at.append(pairRow('', 'neutre', (a, t) => {
      if (!a) return;
      draftTone = false;
      save({ appTones: { ...tones, [a]: t } });
    }, () => { draftTone = false; renderPairs(); }, true));
  }
}
$('addSnippet').addEventListener('click', () => {
  draftSnippet = true;
  renderPairs();
  $('snippets').querySelector('.pair:last-child input')?.focus();
});
$('addAppTone').addEventListener('click', () => {
  draftTone = true;
  renderPairs();
  $('appTones').querySelector('.pair:last-child input')?.focus();
});

/* ---------- Historique ---------- */

const DATE = new Intl.DateTimeFormat('fr-FR', { day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' });

async function loadHistory() {
  const box = $('history');
  let items = [];
  try {
    items = await invoke('history_list');
  } catch {
    items = [];
  }
  box.innerHTML = '';
  if (!items.length) {
    const e = document.createElement('div');
    e.className = 'empty card';
    e.textContent = cfg.privacyMode
      ? 'Mode confidentialité : aucune dictée n’est gardée.'
      : 'Aucune dictée pour l’instant. Appuyez sur votre raccourci et parlez.';
    box.append(e);
    return;
  }
  for (const it of items) {
    const card = document.createElement('article');
    card.className = 'item';
    const meta = document.createElement('div');
    meta.className = 'item-meta';
    const date = document.createElement('span');
    date.className = 'date';
    date.textContent = it.at ? DATE.format(new Date(it.at)) : '';
    meta.append(date);
    const span = (t) => {
      const e = document.createElement('span');
      e.textContent = t;
      return e;
    };
    if (it.app) meta.append(span('·'), span(it.app));
    meta.append(span('·'), span(`${it.words} mot${it.words > 1 ? 's' : ''}`));
    if (it.partial) {
      const b = document.createElement('span');
      b.className = 'badge partial';
      b.textContent = 'en pause, non terminée';
      meta.append(b);
    }
    if (it.recovered) {
      const b = document.createElement('span');
      b.className = 'badge recovered';
      b.textContent = 'rattrapée';
      meta.append(b);
    }
    const copy = document.createElement('button');
    copy.className = 'btn ghost copy';
    copy.textContent = 'Copier';
    copy.addEventListener('click', async () => {
      const ok = await invoke('copy_text', { text: it.text });
      copy.textContent = ok ? 'Copié' : 'Échec';
      setTimeout(() => (copy.textContent = 'Copier'), 1400);
    });
    meta.append(copy);
    const text = document.createElement('div');
    text.className = 'item-text';
    text.textContent = it.text;
    card.append(meta, text);
    box.append(card);
  }
}

/* Vider : une confirmation dans la page, jamais une boîte de dialogue. */
function clearBox(asking) {
  const box = $('clearBox');
  box.innerHTML = '';
  if (!asking) {
    const b = document.createElement('button');
    b.className = 'btn ghost';
    b.textContent = 'Vider l\u2019historique';
    b.addEventListener('click', () => clearBox(true));
    box.append(b);
    return;
  }
  const label = document.createElement('span');
  label.className = 'muted';
  label.textContent = 'Tout effacer ?';
  const no = document.createElement('button');
  no.className = 'btn ghost';
  no.textContent = 'Garder';
  no.addEventListener('click', () => clearBox(false));
  const yes = document.createElement('button');
  yes.className = 'btn danger';
  yes.textContent = 'Effacer';
  yes.addEventListener('click', async () => {
    await invoke('history_clear');
    clearBox(false);
    loadHistory();
    toast('Historique vidé');
  });
  box.append(label, no, yes);
}
clearBox(false);

/* ---------- Cycle de vie ---------- */

async function reload() {
  cfg = await invoke('get_settings');
  render();
  if (!$('tab-history').hidden) loadHistory();
}

listen('settings:tab', (e) => showTab(e.payload));
listen('settings:changed', () => reload());
listen('settings:theme', (e) => applyTheme(e.payload));
window.addEventListener('focus', () => reload());

reload().then(() => {
  showTab((location.hash || '#general').slice(1));
  loadMicrophones();
});
