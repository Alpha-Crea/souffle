'use strict';
const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('souffle', {
  onState: (cb) => ipcRenderer.on('overlay:state', (_e, data) => cb(data)),
  onLevel: (cb) => ipcRenderer.on('overlay:level', (_e, level) => cb(level)),
  togglePause: () => ipcRenderer.send('overlay:toggle-pause'),
  retry: () => ipcRenderer.send('overlay:retry'),
  stop: () => ipcRenderer.send('overlay:stop'),
  cancel: () => ipcRenderer.send('overlay:cancel'),
  dragStart: (pos) => ipcRenderer.send('overlay:drag-start', pos),
  dragMove: (pos) => ipcRenderer.send('overlay:drag-move', pos),
  dragEnd: () => ipcRenderer.send('overlay:drag-end')
});
