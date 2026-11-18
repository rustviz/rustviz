import React from 'react';
import ReactDOM from 'react-dom/client';
import './index.css';
import App from './App';
import { loadTelemetry, TELEMETRY_ENABLED } from './telemetry';

function render() {
  const root = ReactDOM.createRoot(
    document.getElementById('root') as HTMLElement
  );
  root.render(
    <React.StrictMode>
      <App />
    </React.StrictMode>
  );
}

// Usage logging is off unless the build set VITE_RV_TELEMETRY_ENDPOINT
// (see telemetry.ts); then wait briefly for its script so the first
// render's figure is already named.
if (TELEMETRY_ENABLED) void loadTelemetry().then(render);
else render();
