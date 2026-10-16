'use strict';
/**
 * File de sécurité des dictées.
 *
 * Chaque morceau d'audio est gardé dès son arrivée et n'est jeté qu'une fois
 * son texte versé dans l'historique. Une panne de réseau, un fournisseur qui
 * refuse, un VPN qui bloque, un plantage : rien de ce qui a été dit n'est
 * perdu, et la dictée peut être retranscrite plus tard.
 *
 * Deux niveaux :
 *   - en mémoire, toujours : c'est ce qui sert au « Réessayer » immédiat ;
 *   - sur le disque, sauf en mode confidentialité : c'est ce qui survit à un
 *     plantage ou à un redémarrage. Le mode confidentialité promet qu'aucune
 *     dictée n'est écrite sur le disque, et cette promesse passe avant.
 */
const fs = require('node:fs');
const path = require('node:path');

const extFor = (mime = '') => (mime.includes('ogg') ? 'ogg' : mime.includes('mp4') ? 'mp4' : 'webm');
const baseName = (id, index) => `${id}-${String(index).padStart(3, '0')}`;

class Pending {
  constructor(dir) {
    this.dir = dir;
    /** id de dictée -> Map(index -> { buffer, mime, durationMs }) */
    this.memory = new Map();
  }

  /** Garde un morceau. `persist` vaut false en mode confidentialité. */
  save(id, index, { buffer, mime, durationMs }, meta = {}, persist = true) {
    const audio = Buffer.from(buffer);
    if (!this.memory.has(id)) this.memory.set(id, new Map());
    this.memory.get(id).set(index, { buffer: audio, mime, durationMs });
    if (!persist) return;
    try {
      fs.mkdirSync(this.dir, { recursive: true });
      const base = path.join(this.dir, baseName(id, index));
      fs.writeFileSync(`${base}.${extFor(mime)}`, audio);
      // Les métadonnées en dernier : un morceau sans .json n'est jamais relu à moitié écrit.
      fs.writeFileSync(`${base}.json`, JSON.stringify({ id, index, mime, durationMs, at: Date.now(), ...meta }));
    } catch (err) {
      console.error('[souffle] file de sécurité : écriture impossible :', err.message);
    }
  }

  /** Morceaux d'une dictée, dans l'ordre : mémoire d'abord, disque sinon. */
  parts(id) {
    const mem = this.memory.get(id);
    if (mem && mem.size) {
      return [...mem.entries()].sort((a, b) => a[0] - b[0]).map(([index, a]) => ({ index, ...a }));
    }
    return this.onDisk()
      .filter((m) => m.id === id)
      .map((m) => ({ index: m.index, mime: m.mime, durationMs: m.durationMs, buffer: this.readAudio(m) }))
      .filter((p) => p.buffer && p.buffer.length);
  }

  readAudio(meta) {
    try {
      return fs.readFileSync(path.join(this.dir, `${baseName(meta.id, meta.index)}.${extFor(meta.mime)}`));
    } catch {
      return null;
    }
  }

  /** Métadonnées de tout ce qui attend sur le disque. */
  onDisk() {
    let files = [];
    try {
      files = fs.readdirSync(this.dir).filter((f) => f.endsWith('.json'));
    } catch {
      return [];
    }
    const metas = [];
    for (const f of files) {
      try {
        metas.push(JSON.parse(fs.readFileSync(path.join(this.dir, f), 'utf8')));
      } catch {
        /* métadonnée illisible : ignorée, le reste suit */
      }
    }
    return metas.sort((a, b) => (a.id === b.id ? a.index - b.index : a.at - b.at));
  }

  /** Identifiants des dictées restées sur le disque (plantage, redémarrage). */
  dictationsOnDisk() {
    const seen = new Map();
    for (const m of this.onDisk()) if (!seen.has(m.id)) seen.set(m.id, m);
    return [...seen.values()];
  }

  /** Un morceau est arrivé à bon port (mode direct) : on oublie son seul audio. */
  forgetPart(id, index) {
    this.memory.get(id)?.delete(index);
    const base = baseName(id, index);
    let files = [];
    try {
      files = fs.readdirSync(this.dir);
    } catch {
      return;
    }
    for (const f of files) {
      if (!f.startsWith(`${base}.`)) continue;
      try {
        fs.unlinkSync(path.join(this.dir, f));
      } catch {
        /* déjà supprimé */
      }
    }
  }

  /** La dictée est arrivée à bon port : on oublie son audio, partout. */
  forget(id) {
    this.memory.delete(id);
    let files = [];
    try {
      files = fs.readdirSync(this.dir);
    } catch {
      return;
    }
    for (const f of files) {
      if (!f.startsWith(`${id}-`)) continue;
      try {
        fs.unlinkSync(path.join(this.dir, f));
      } catch {
        /* déjà supprimé */
      }
    }
  }
}

module.exports = { Pending };
