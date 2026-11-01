# Launch Electron wrapper for HTML plugins
# 
# Prerequisites:
# - Electron must be installed at C:\Program Files (x86)\electron\electron.exe
#   or in your PATH as 'electron.exe'
#
# Usage:
#   powershell.exe -ExecutionPolicy Bypass -File launch-electron-plugin.ps1 -PluginUuid <uuid> -PluginDir <path> -WsPort <port>

param(
    [Parameter(Mandatory=$true)]
    [string]$PluginUuid,
    
    [Parameter(Mandatory=$true)]
    [string]$PluginDir,
    
    [Parameter(Mandatory=$true)]
    [int]$WsPort
)

$ELECTRON_PATH = "C:\Program Files (x86)\electron\electron.exe"
$ALT_ELECTRON_PATH = "C:\Program Files\electron\electron.exe"
$LOCAL_ELECTRON = "$env:LOCALAPPDATA\electron\electron.exe"

# Check for Electron installation
if (Test-Path $ELECTRON_PATH) {
    $ELECTRON_EXE = $ELECTRON_PATH
} elseif (Test-Path $ALT_ELECTRON_PATH) {
    $ELECTRON_EXE = $ALT_ELECTRON_PATH
} elseif (Test-Path $LOCAL_ELECTRON) {
    $ELECTRON_EXE = $LOCAL_ELECTRON
} elseif (Get-Command "electron.exe" -ErrorAction SilentlyContinue) {
    $ELECTRON_EXE = "electron.exe"
} else {
    Write-Host "[electron-wrapper] ERROR: Electron no está instalado" -ForegroundColor Red
    Write-Host "[electron-wrapper] Descárgalo de: https://www.electronjs.org/docs/latest/tutorial/quick-start" -ForegroundColor Yellow
    Write-Host "[electron-wrapper] O instala con: npm install -g electron" -ForegroundColor Yellow
    exit 1
}

$HTML_PATH = Join-Path $PSScriptRoot "plugin-page.html"

Write-Host "[electron-wrapper] Launching Electron for plugin: $PluginUuid" -ForegroundColor Cyan
Write-Host "[electron-wrapper] Port: $WsPort"
Write-Host "[electron-wrapper] Plugin dir: $PluginDir"

# Create the HTML page that loads the plugin
@"
<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <title>Stream Deck Plugin - $PluginUuid</title>
    <style>
        body { margin: 0; padding: 0; overflow: hidden; background: #1a1a1a; color: #fff; font-family: Arial, sans-serif; }
        #plugin-container { padding: 20px; }
    </style>
</head>
<body>
    <div id="plugin-container">
        <h2>Plugin: $PluginUuid</h2>
        <p>WebSocket port: $WsPort</p>
        <p>Cargando contenido del plugin...</p>
        <div id="plugin-content"></div>
    </div>
    
    <script>
        const pluginUuid = '$PluginUuid';
        const wsPort = $WsPort;
        const pluginDir = '$PluginDir'.replace(/\\/g, '/');
        
        // WebSocket connection to host
        const ws = new WebSocket('ws://127.0.0.1:' + wsPort);
        
        ws.onopen = () => {
            console.log('[plugin] WebSocket connected to port ' + wsPort);
            
            // This is where real plugins would register
            // connectElgatoStreamDeckSocket(wsPort, pluginUuid, 'registerPlugin', info);
        };
        
        ws.onmessage = (event) => {
            const data = JSON.parse(event.data);
            console.log('[plugin] Received from host:', data);
            
            if (data.event === 'willAppear') {
                console.log('[plugin] Key will appear on context:', data.context);
            } else if (data.event === 'keyDown') {
                console.log('[plugin] Key pressed:');
                // Handle key press
            } else if (data.event === 'keyUp') {
                console.log('[plugin] Key released:');
                // Handle key release
            }
        };
        
        ws.onerror = (err) => {
            console.error('[plugin] WebSocket error:', err);
        };
        
        ws.onclose = () => {
            console.log('[plugin] WebSocket disconnected');
        };
        
        // Try to load the actual plugin HTML
        fetch(pluginDir + '/index.html')
            .then(response => response.text())
            .then(html => {
                document.getElementById('plugin-content').innerHTML = html;
                console.log('[plugin] Loaded plugin content from', pluginDir);
            })
            .catch(err => {
                console.log('[plugin] Using default view (no index.html found)');
                document.getElementById('plugin-content').innerHTML = 
                    '<p>Waiting for events from host...</p>';
            });
    </script>
</body>
</html>
"@ | Out-File -FilePath $HTML_PATH -Encoding UTF8

# Launch Electron
try {
    Start-Process -FilePath $ELECTRON_EXE -ArgumentList $HTML_PATH
    Write-Host "[electron-wrapper] Electron launched successfully" -ForegroundColor Green
} catch {
    Write-Host "[electron-wrapper] ERROR: Failed to launch Electron: $_" -ForegroundColor Red
}