'use strict';

const $ = (id) => document.getElementById(id);
const api = window.souffle;

let cfg = null;

/* --------------------------------- outils --------------------------------- */

function toast(msg) {
  const el = $('toast');
  el.textContent = msg;
  el.classList.add('show');
  clearTimeout(toast._t);
  toast._t = setTimeout(() => el.classList.remove('show'), 1900);
}

const save = (patch) => api.set(patch);

/** Interrupteurs simples : même liste pour l'affichage initial et pour l'écoute. */
const TOGGLES = [
  'autoPaste',
  'restoreClipboard',
  'refocusTarget',
  'playSounds',
  'launchAtLogin',
  'formatEnabled',
  'privacyMode',
  'liveMode'
];

function bind(id, key, { type = 'value', parse = (v) => v } = {}) {
  const el = $(id);
  if (!el) return;
  const event = type === 'checked' ? 'change' : el.tagName === 'SELECT' ? 'change' : 'input';
  let timer = null;
  el.addEventListener(event, () => {
    const value = parse(type === 'checked' ? el.checked : el.value);
    clearTimeout(timer);
    timer = setTimeout(() => save({ [key]: value }), event === 'input' ? 400 : 0);
  });
}

/* ------------------------------- navigation ------------------------------- */

document.querySelectorAll('.nav-item').forEach((btn) => {
  btn.addEventListener('click', () => openTab(btn.dataset.tab));
});

function openTab(tab) {
  document.querySelectorAll('.nav-item').forEach((b) => b.classList.toggle('active', b.dataset.tab === tab));
  document.querySelectorAll('.tab').forEach((s) => s.classList.toggle('active', s.id === `tab-${tab}`));
  if (tab === 'history') renderHistory();
}

api.onTab(openTab);

/* -------------------------------- raccourci ------------------------------- */

const MODIFIERS = new Set(['Control', 'Shift', 'Alt', 'Meta']);

function accelerator(e) {
  const parts = [];
  if (e.metaKey) parts.push('Command');
  if (e.ctrlKey) parts.push('Control');
  if (e.altKey) parts.push('Alt');
  if (e.shiftKey) parts.push('Shift');

  let key = e.key;
  if (MODIFIERS.has(key)) return null;
  if (key === ' ' || e.code === 'Space') key = 'Space';
  else if (/^F\d{1,2}$/.test(key)) key = key;
  else if (key.length === 1) key = key.toUpperCase();
  else if (['Escape', 'Tab', 'Backspace'].includes(key)) return null;

  parts.push(key);
  // Une touche seule n'est acceptée que si c'est une touche de fonction.
  // `startsWith('F')` laissait aussi passer la lettre F : appuyer dessus
  // enregistrait le raccourci « F » — sans modificateur, donc exactement ce
  // que Souffle s'interdit, et sur macOS de quoi faire taire le raccourci
  // principal en cassant le gestionnaire de hotkeys du système.
  const isFunctionKey = /^F\d{1,2}$/.test(key);
  return parts.length > 1 || isFunctionKey ? parts.join('+') : null;
}

const shortcutInput = $('shortcut');

shortcutInput.addEventListener('focus', () => shortcutInput.classList.add('capturing'));
shortcutInput.addEventListener('blur', () => {
  shortcutInput.classList.remove('capturing');
  shortcutInput.value = cfg.shortcut;
});

shortcutInput.addEventListener('keydown', async (e) => {
  e.preventDefault();
  const accel = accelerator(e);
  if (!accel) return;
  const { taken } = await api.validateShortcut(accel);
  if (taken && accel !== cfg.shortcut) return toast(`« ${accel} » est déjà utilisé`);
  cfg.shortcut = accel;
  shortcutInput.value = accel;
  await save({ shortcut: accel });
  shortcutInput.blur();
  toast('Raccourci mis à jour');
});

/* --------------------------------- clés API ------------------------------- */

function paintSecret(key, present) {
  const badge = $(`badge-${key}`);
  badge.textContent = present ? 'enregistrée' : 'absente';
  badge.className = `badge ${present ? 'ok' : ''}`;
  if (present) $(key).placeholder = '••••••••••••••••';
}

['openaiKey', 'groqKey', 'customKey'].forEach((key) => {
  let timer = null;
  $(key).addEventListener('input', (e) => {
    clearTimeout(timer);
    timer = setTimeout(async () => {
      const status = await api.setSecret(key, e.target.value.trim());
      paintSecret(key, status[key]);
      if (e.target.value.trim()) toast('Clé chiffrée et enregistrée');
    }, 600);
  });
});

document.querySelectorAll('[data-test]').forEach((btn) => {
  btn.addEventListener('click', async () => {
    const provider = btn.dataset.test;
    btn.disabled = true;
    btn.textContent = '…';
    const customUrl = provider === 'custom' ? $('customSttUrl').value || $('customLlmUrl').value : '';
    const res = await api.testSecret(provider, $(`${provider}Key`).value.trim(), customUrl);
    const out = $('test-result');
    out.textContent = res.ok
      ? `✓ ${provider} répond — ${res.models} modèles disponibles.`
      : `✗ ${provider} : ${res.error}`;
    out.style.color = res.ok ? 'var(--ok)' : 'var(--bad)';
    btn.disabled = false;
    btn.textContent = 'Tester';
  });
});

/* ------------------------------- paires (UI) ------------------------------ */

function renderPairs(container, items, fields, onChange) {
  container.innerHTML = '';
  items.forEach((item, i) => {
    const row = document.createElement('div');
    row.className = 'pair';

    fields.forEach((f) => {
      let el;
      if (f.options) {
        el = document.createElement('select');
        f.options.forEach(([v, label]) => {
          const o = document.createElement('option');
          o.value = v;
          o.textContent = label;
          el.append(o);
        });
      } else {
        el = document.createElement('input');
        el.placeholder = f.placeholder || '';
      }
      el.value = item[f.key] ?? '';
      el.addEventListener('input', () => {
        items[i][f.key] = el.value;
        onChange(items);
      });
      el.addEventListener('change', () => {
        items[i][f.key] = el.value;
        onChange(items);
      });
      row.append(el);
    });

    const del = document.createElement('button');
    del.textContent = '×';
    del.title = 'Supprimer';
    del.addEventListener('click', () => {
      items.splice(i, 1);
      onChange(items);
      renderPairs(container, items, fields, onChange);
    });
    row.append(del);
    container.append(row);
  });
}

let snippets = [];
let appTones = [];

const TONE_OPTIONS = [
  ['brut', 'Brut'],
  ['neutre', 'Neutre'],
  ['formel', 'Formel'],
  ['casual', 'Détendu']
];

function refreshSnippets() {
  renderPairs(
    $('snippets'),
    snippets,
    [
      { key: 'trigger', placeholder: 'mon email' },
      { key: 'value', placeholder: 'leo.sperl@alphagency.fr' }
    ],
    (items) => save({ snippets: items.filter((s) => s.trigger || s.value) })
  );
}

function refreshAppTones() {
  renderPairs(
    $('appTones'),
    appTones,
    [
      { key: 'app', placeholder: 'Slack' },
      { key: 'tone', options: TONE_OPTIONS }
    ],
    (items) => {
      const map = {};
      items.forEach((r) => r.app && (map[r.app] = r.tone || 'neutre'));
      save({ appTones: map });
    }
  );
}

$('addSnippet').addEventListener('click', () => {
  snippets.push({ trigger: '', value: '' });
  refreshSnippets();
});

$('addAppTone').addEventListener('click', () => {
  appTones.push({ app: '', tone: 'neutre' });
  refreshAppTones();
});

/* -------------------------------- historique ------------------------------ */

async function renderHistory() {
  const items = await api.history();
  const box = $('historyList');
  box.innerHTML = '';
  if (!items.length) {
    box.innerHTML = '<div class="note">Aucune dictée enregistrée pour le moment.</div>';
    return;
  }
  items.forEach((it) => {
    const el = document.createElement('div');
    el.className = 'entry';

    const meta = document.createElement('div');
    meta.className = 'meta';
    const when = new Date(it.at).toLocaleString('fr-FR', { dateStyle: 'short', timeStyle: 'short' });
    meta.textContent = [when, it.app, `${it.words} mots`].filter(Boolean).join(' · ');

    const body = document.createElement('div');
    body.className = 'body';
    body.textContent = it.text;

    const copy = document.createElement('button');
    copy.textContent = 'Copier';
    // navigator.clipboard exige que le document ait le focus et rejette en
    // silence quand ce n'est pas le cas : le bouton semblait alors marcher sans
    // rien copier. Le presse-papier d'Electron, côté process principal, n'a ni
    // cette contrainte ni ce mode d'échec muet.
    copy.addEventListener('click', async () => {
      try {
        await api.copyText(it.text);
        toast('Copié');
      } catch (err) {
        toast('Copie impossible');
        console.error('copie échouée :', err);
      }
    });

    el.append(meta, body, copy);
    box.append(el);
  });
}

$('clearHistory').addEventListener('click', async () => {
  await api.clearHistory();
  renderHistory();
  toast('Historique vidé');
});

/* ---------------------------------- stats --------------------------------- */

function renderStats(stats) {
  const spokenMin = stats.seconds / 60;
  const typedMin = stats.words / 45; // vitesse de frappe moyenne
  const saved = Math.max(0, Math.round(typedMin - spokenMin));
  $('stats').innerHTML = '';
  [
    [stats.words.toLocaleString('fr-FR'), 'mots dictés'],
    [stats.sessions.toLocaleString('fr-FR'), 'dictées'],
    [`${saved} min`, 'gagnées vs clavier']
  ].forEach(([value, label]) => {
    const d = document.createElement('div');
    d.className = 'stat';
    const b = document.createElement('b');
    b.textContent = value;
    const s = document.createElement('span');
    s.textContent = label;
    d.append(b, s);
    $('stats').append(d);
  });
}

/* -------------------------------- autres UI ------------------------------- */

$('try').addEventListener('click', () => api.dictate());
$('askMic').addEventListener('click', async () => {
  const ok = await api.askMicrophone();
  toast(ok ? 'Micro autorisé' : 'Micro refusé — vérifiez les réglages système');
});
$('askA11y').addEventListener('click', () => api.askAccessibility());

function syncProviderRows() {
  $('row-sttUrl').classList.toggle('hidden', $('sttProvider').value !== 'custom');
  $('row-llmUrl').classList.toggle('hidden', $('llmProvider').value !== 'custom');
}
$('sttProvider').addEventListener('change', syncProviderRows);
$('llmProvider').addEventListener('change', syncProviderRows);

function syncOutputMode() {
  const translating = $('outputMode').value === 'translate';
  $('row-target').classList.toggle('hidden', !translating);
  $('output-note').textContent = translating
    ? 'Traduction : parlez dans n’importe quelle langue, le texte sort dans celle choisie.'
    : 'Transcription fidèle : anglais parlé → anglais écrit, corrigé et ponctué. Aucune traduction.';
}
$('outputMode').addEventListener('change', syncOutputMode);

/* -------------------------------- démarrage ------------------------------- */

async function init() {
  cfg = await api.get();

  $('version').textContent = `v${cfg.version}`;
  shortcutInput.value = cfg.shortcut;

  const simple = [
    'mode', 'escapeAction', 'sttProvider', 'sttModel', 'customSttUrl', 'language',
    'outputMode', 'targetLanguage',
    'llmProvider', 'llmModel', 'customLlmUrl', 'tone', 'customInstructions'
  ];
  simple.forEach((k) => ($(k).value = cfg[k] ?? ''));

  TOGGLES.forEach((k) => ($(k).checked = Boolean(cfg[k])));
  $('historyLimit').value = cfg.historyLimit;
  $('dictionary').value = (cfg.dictionary || []).join('\n');

  simple.forEach((k) => bind(k, k));
  TOGGLES.forEach((k) => bind(k, k, { type: 'checked' }));
  bind('historyLimit', 'historyLimit', { parse: (v) => Math.max(0, parseInt(v, 10) || 0) });
  bind('dictionary', 'dictionary', {
    parse: (v) => v.split('\n').map((s) => s.trim()).filter(Boolean)
  });

  snippets = (cfg.snippets || []).map((s) => ({ ...s }));
  appTones = Object.entries(cfg.appTones || {}).map(([app, tone]) => ({ app, tone }));
  refreshSnippets();
  refreshAppTones();

  Object.entries(cfg.secrets).forEach(([k, present]) => paintSecret(k, present));
  renderStats(cfg.stats);
  syncProviderRows();
  syncOutputMode();

  $('escape-note').textContent = cfg.escapeAvailable
    ? 'Échap n’agit que pendant une dictée, jamais le reste du temps.'
    : 'Indisponible sur cette installation — utilisez le raccourci ou la pilule.';
  $('escapeAction').disabled = !cfg.escapeAvailable;

  // Le mode « maintenir » demande un module natif optionnel.
  $('mode-note').textContent = cfg.holdAvailable
    ? 'Le mode « maintenir » est disponible.'
    : 'Installez uiohook-napi pour activer « maintenir pour parler ».';
  if (!cfg.holdAvailable) $('mode').querySelector('option[value="hold"]').disabled = true;

  // Accessibilité : macOS uniquement.
  if (cfg.platform !== 'darwin') {
    $('row-accessibility').classList.add('hidden');
  } else {
    const badge = $('a11y-badge');
    badge.textContent = cfg.accessibility ? 'accordée' : 'requise';
    badge.className = `badge ${cfg.accessibility ? 'ok' : 'bad'}`;
  }

  if (!cfg.secrets.openaiKey && !cfg.secrets.groqKey) openTab('ai');
}

init();
