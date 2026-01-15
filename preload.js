const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('electron', {
  startLogin: (host) => ipcRenderer.send('start-login', host),
  onLoginComplete: (callback) => ipcRenderer.on('login-complete', (_, data) => callback(data))
});
