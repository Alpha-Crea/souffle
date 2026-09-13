'use strict';
const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('souffle', {
  get: () => ipcRenderer.invoke('settings:get'),
  set: (patch) => ipcRenderer.invoke('settings:set', patch),
  setSecret: (key, value) => ipcRenderer.invoke('secret:set', { key, value }),
  testSecret: (provider, key, customUrl) =>
    ipcRenderer.invoke('secret:test', { provider, key, customUrl }),
  validateShortcut: (accel) => ipcRenderer.invoke('shortcut:validate', accel),
  copyText: (text) => ipcRenderer.invoke('app:copyText', text),
  history: () => ipcRenderer.invoke('history:list'),
  clearHistory: () => ipcRenderer.invoke('history:clear'),
  askAccessibility: () => ipcRenderer.invoke('perm:accessibility'),
  askMicrophone: () => ipcRenderer.invoke('perm:microphone'),
  dictate: () => ipcRenderer.invoke('app:dictate'),
  openExternal: (url) => ipcRenderer.invoke('app:openExternal', url),
  onTab: (cb) => ipcRenderer.on('settings:tab', (_e, tab) => cb(tab))
});
