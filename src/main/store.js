'use strict';
/**
 * Persistance JSON simple + chiffrement des clés API via safeStorage (Keychain / DPAPI).
 * Aucune dépendance externe : le fichier vit dans app.getPath('userData').
 */
const fs = require('node:fs');
const path = require('node:path');
const { app, safeStorage } = require('electron');

const DEFAULTS = {
  // Déclenchement
  // Windows réserve Ctrl+Espace au changement de langue du clavier, et Alt+Espace
  // ouvre le menu système de la fenêtre : ni l'un ni l'autre n'est utilisable.
  shortcut: process.platform === 'darwin' ? 'Alt+Space' : 'Control+Shift+Space',
  mode: 'toggle', // 'toggle' | 'hold'  (hold nécessite uiohook-napi)
  escapeAction: 'stop', // Échap pendant la dictée : 'stop' | 'cancel' | 'off'
  liveMode: false, // écriture phrase par phrase pendant qu'on parle
  playSounds: true,
  autoPaste: true,
  // Restaurer l'ancien presse-papier après le collage fait perdre la dictée dès
  // que le collage a raté — application sans champ de saisie, fenêtre changée
  // entre-temps. Tant que le texte y reste, un ⌘V manuel rattrape n'importe
  // quel échec. D'où : désactivé par défaut.
  restoreClipboard: false,
  refocusTarget: true,
  launchAtLogin: false,

  // Transcription
  sttProvider: 'openai', // 'openai' | 'groq' | 'custom'
  sttModel: 'gpt-4o-mini-transcribe',
  customSttUrl: '',
  language: 'auto', // langue PARLÉE attendue : 'auto' | 'fr' | 'en' | ...

  // Langue de sortie — c'est un choix explicite, jamais une devinette du modèle.
  outputMode: 'same', // 'same' = on écrit dans la langue parlée | 'translate'
  targetLanguage: 'fr', // utilisé uniquement si outputMode === 'translate'

  // Reformulation
  formatEnabled: true,
  llmProvider: 'openai', // 'openai' | 'groq' | 'custom'
  llmModel: 'gpt-4o-mini',
  customLlmUrl: '',
  tone: 'neutre', // 'brut' | 'neutre' | 'formel' | 'casual'
  appTones: {}, // { "Slack": "casual", "Mail": "formel" }

  // Personnalisation
  dictionary: [], // ["Alpha Agency", "KACHÉ", "Photon Fusion"]
  snippets: [], // [{ trigger: "mon email", value: "leo@..." }]
  customInstructions: '',

  // Confidentialité
  privacyMode: false, // n'enregistre aucun historique
  historyLimit: 100,

  // Stats
  stats: { words: 0, sessions: 0, seconds: 0 },

  // Divers
  onboarded: false
};

const SECRET_KEYS = ['openaiKey', 'groqKey', 'customKey'];

class Store {
  constructor() {
    this.file = path.join(app.getPath('userData'), 'config.json');
    this.data = { ...DEFAULTS };
    this.secrets = {};
    this.load();
  }

  load() {
    try {
      const raw = JSON.parse(fs.readFileSync(this.file, 'utf8'));
      const { __secrets, ...rest } = raw;
      this.data = { ...DEFAULTS, ...rest };
      if (__secrets) {
        for (const k of SECRET_KEYS) {
          if (!__secrets[k]) continue;
          try {
            this.secrets[k] = safeStorage.isEncryptionAvailable()
              ? safeStorage.decryptString(Buffer.from(__secrets[k], 'base64'))
              : Buffer.from(__secrets[k], 'base64').toString('utf8');
          } catch {
            this.secrets[k] = '';
          }
        }
      }
    } catch {
      /* premier lancement */
    }
  }

  save() {
    const __secrets = {};
    for (const k of SECRET_KEYS) {
      const v = this.secrets[k];
      if (!v) continue;
      __secrets[k] = safeStorage.isEncryptionAvailable()
        ? safeStorage.encryptString(v).toString('base64')
        : Buffer.from(v, 'utf8').toString('base64');
    }
    const tmp = this.file + '.tmp';
    fs.mkdirSync(path.dirname(this.file), { recursive: true });
    fs.writeFileSync(tmp, JSON.stringify({ ...this.data, __secrets }, null, 2));
    fs.renameSync(tmp, this.file);
  }

  get(key) {
    return this.data[key];
  }

  all() {
    return { ...this.data };
  }

  set(patch) {
    Object.assign(this.data, patch);
    this.save();
  }

  getSecret(key) {
    return this.secrets[key] || '';
  }

  setSecret(key, value) {
    if (!SECRET_KEYS.includes(key)) return;
    this.secrets[key] = value || '';
    this.save();
  }

  /** Quelles clés sont renseignées (sans jamais exposer leur valeur au renderer). */
  secretStatus() {
    const out = {};
    for (const k of SECRET_KEYS) out[k] = Boolean(this.secrets[k]);
    return out;
  }

  bumpStats({ words = 0, seconds = 0 }) {
    const s = this.data.stats;
    s.words += words;
    s.seconds += seconds;
    s.sessions += 1;
    this.save();
  }
}

module.exports = { Store, DEFAULTS, SECRET_KEYS };
