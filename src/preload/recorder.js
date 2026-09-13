'use strict';
const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('souffle', {
  onStart: (cb) => ipcRenderer.on('recorder:start', (_e, opts) => cb(opts || {})),
  onStop: (cb) => ipcRenderer.on('recorder:stop', () => cb()),
  onCancel: (cb) => ipcRenderer.on('recorder:cancel', () => cb()),
  sendAcquiring: () => ipcRenderer.send('recorder:acquiring'),
  sendStarted: () => ipcRenderer.send('recorder:started'),
  sendAudio: (payload) => ipcRenderer.send('recorder:audio', payload),
  sendLevel: (level) => ipcRenderer.send('recorder:level', level),
  sendError: (message) => ipcRenderer.send('recorder:error', message)
});
