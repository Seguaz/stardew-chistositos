# Stardew Chistositos — Lanzador (Tauri)

Lanzador de escritorio para la granja co-op modeada. Comprueba la versión del pack
de mods en el servidor, lo descarga/actualiza si hace falta (con verificación SHA256),
y lanza el juego con SMAPI. Un solo botón para los amigos.

## Estructura

- `src/` — interfaz (React + GSAP). UI principal en `src/App.tsx`, estilos en `src/App.css`.
- `src-tauri/src/lib.rs` — backend en Rust (detección de carpeta, descarga, extracción, lanzar SMAPI).
- `src-tauri/tauri.conf.json` — configuración de la ventana e instalador.

El servidor sirve dos archivos en `https://stardew.seguaz.online/`:
- `manifest.json` — `{ version, smapi, gameVersion, file, sha256, sizeBytes }`
- `ClientMods.zip` — carpetas de mods en la raíz (se extraen sobre la carpeta `Mods/` del cliente).

## Desarrollo

```powershell
# ventana en vivo con recarga
npm run tauri dev
```

## Compilar el instalador

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
npm run tauri build
```

El instalador NSIS queda en:
`src-tauri\target\release\bundle\nsis\Stardew Chistositos_<version>_x64-setup.exe`

Ese `.exe` es lo que se reparte a los amigos. Instala WebView2 automáticamente si falta.

## Cómo actualizar el pack de mods (cuando cambies mods en el server)

1. Regenera `ClientMods.zip` con las carpetas de mods del cliente en la raíz del zip.
2. Calcula el SHA256 y el tamaño en bytes.
3. Sube el zip a `~/stardew/launcher_pack/` en el servidor.
4. Actualiza `manifest.json` con la nueva `version` (formato `AAAA.MM.DD.n`), `sha256` y `sizeBytes`.

Los amigos no reinstalan el lanzador: al abrirlo detecta la versión nueva y descarga solo el pack.

## Para los amigos (instrucciones)

1. Descarga e instala `Stardew Chistositos_..._x64-setup.exe`.
2. Ábrelo. Detecta tu carpeta de Stardew automáticamente (Steam/GOG). Si no, pega la ruta.
3. Pulsa **JUGAR**. La primera vez descarga el pack (~140 MB); después solo si hay actualización.
4. En el menú del juego: **Co-op → Unirse → IP del servidor**.
