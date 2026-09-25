import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { BrowserRouter } from 'react-router'
import { App } from './App.jsx'
import { AppProvider } from './AppProvider.jsx'
import { removeLegacyServiceWorkers } from './lib/sw-cleanup.js'
import './index.css'

removeLegacyServiceWorkers()

createRoot(document.getElementById('root')).render(
  <StrictMode>
    <BrowserRouter>
      <AppProvider>
        <App />
      </AppProvider>
    </BrowserRouter>
  </StrictMode>,
)
