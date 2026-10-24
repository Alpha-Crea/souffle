'use strict';
/** Historique local des dictées (désactivé en mode confidentialité). */
const fs = require('node:fs');
const path = require('node:path');
const { app } = require('electron');

class History {
  constructor(dir = app.getPath('userData')) {
    this.file = path.join(dir, 'history.json');
    this.items = [];
    try {
      this.items = JSON.parse(fs.readFileSync(this.file, 'utf8'));
    } catch {
      this.items = [];
    }
  }

  add(entry, limit = 100) {
    this.upsert(`h${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`, entry, limit);
  }

  /**
   * Ajoute une entrée, ou remplace celle qui porte déjà cet identifiant.
   * Une dictée mise en pause est versée tout de suite, puis remplacée par son
   * texte final : elle n'apparaît qu'une fois, à la place qu'elle avait.
   */
  upsert(id, entry, limit = 100) {
    const i = this.items.findIndex((x) => x.id === id);
    if (i >= 0) {
      this.items[i] = { ...entry, id, at: this.items[i].at };
    } else {
      this.items.unshift({ ...entry, id, at: Date.now() });
      if (this.items.length > limit) this.items.length = Math.max(0, limit);
    }
    this.flush();
  }

  list() {
    return this.items;
  }

  clear() {
    this.items = [];
    this.flush();
  }

  flush() {
    try {
      const tmp = `${this.file}.tmp`;
      fs.writeFileSync(tmp, JSON.stringify(this.items));
      fs.renameSync(tmp, this.file);
    } catch {
      /* disque en lecture seule : on ignore */
    }
  }
}

module.exports = { History };
