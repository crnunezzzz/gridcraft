# GridCraft IA para Windows

> Estado · 2026-11-10: asistente de escritorio en Windows x64 y Ubuntu / Linux x64 (también
> compila en macOS), con aviso de versiones nuevas y «Actualizar» en un toque en Windows, Linux y
> Android. Las pruebas automáticas pasan; falta probar Linux a mano en un PC con Ubuntu.

GridCraft IA es GridCraft con el asistente de IA de la versión Android, en el escritorio. El
asistente se abre desde la pestaña **Asistente** de la cinta. Lee el libro abierto y lo edita
con las mismas órdenes que la línea de comandos.

## Conectar la IA

En el asistente › engranaje › **Conexión con la IA**, cada proveedor se conecta de una de dos formas:

| Forma | Qué hace | Qué necesitas |
|---|---|---|
| **Clave de API** (OpenAI o Anthropic) | La app envía las peticiones directamente a la API del proveedor. | Una clave con saldo. Se guarda en el Administrador de credenciales de Windows y solo viaja a su proveedor. |
| **Plan de ChatGPT** | La app usa **Codex**, la herramienta oficial de OpenAI, con tu sesión de ChatGPT. | Un plan de ChatGPT que incluya Codex. |
| **Plan de Claude** | La app usa **Claude Code**, la herramienta oficial de Anthropic, con tu sesión de Claude. | Un plan de Claude que incluya Claude Code (Pro, Max, Team o Enterprise). |

Con un plan, **la app nunca ve tu contraseña ni tus credenciales**. «Instalar» abre una ventana
con el instalador oficial del proveedor, e «Iniciar sesión» abre su inicio de sesión, que sigue
en el navegador. La app arranca la herramienta tal como la instala el proveedor y lee sus
respuestas. Las peticiones cuentan en el uso de tu plan.

### Qué puede hacer la IA con un plan

La herramienta se arranca sin herramientas propias: ni terminal, ni archivos, ni web, ni
parches. Solo recibe las dos herramientas del documento (`list_commands` y `execute_command`), a
través de un servidor MCP que es la propia app (`gridcraft.exe --assistant-mcp`). Ese servidor
habla con la ventana abierta por un socket local (127.0.0.1) protegido con un token aleatorio. Se
aplican la misma lista de órdenes permitidas y los mismos límites que con la clave de API: sin
rutas de archivo, credenciales, macros ni procesos.

### Diferencias con la clave de API

- **Codex (plan de ChatGPT)** lee imágenes y el texto de los documentos adjuntos, pero no PDF.
  Con la clave de API de OpenAI, el PDF se envía entero.
- Con un plan, los adjuntos de un mensaje pueden ocupar hasta 9 MB (con la clave de API, 28 MB).
- «Razonamiento» (bajo, medio, alto) se aplica a Codex y a la API. Claude Code usa su propio ajuste.

### Por qué no hay un «Iniciar sesión con Claude» dentro de la app

Anthropic no permite que otras aplicaciones ofrezcan el inicio de sesión de Claude.ai ni que
guarden o reutilicen sus tokens. Por eso la app no lo hace: usa tu Claude Code sin modificar, con
la sesión que tú iniciaste en él. Con ChatGPT pasa lo mismo: la app no reutiliza el inicio de
sesión de Codex, sino que usa tu Codex con tu propia sesión.

## Instalar y actualizar sin que el launcher lo rompa

El launcher de Craft Apps instala y actualiza el **GridCraft oficial**, que no trae el asistente.
Si se reemplazaran los archivos del GridCraft oficial, la siguiente actualización del launcher
los sobrescribiría. Por eso GridCraft IA:

1. **Se instala aparte**, en `%LOCALAPPDATA%\Programs\GridCraft IA`, con su propio acceso
   directo «GridCraft IA». El launcher no toca esa carpeta, y GridCraft IA tampoco toca la del
   oficial. Los dos pueden convivir y comparten ajustes y documentos recientes.
2. **Recibe las mejoras del proyecto original** con el workflow
   [`ia-windows.yml`](../.github/workflows/ia-windows.yml). Cada día fusiona
   `storytold/gridcraft` en `main`, prueba el asistente sobre el código fusionado y publica una
   versión nueva en *Releases*: el zip de Windows, el zip de Linux y la APK firmada de Android
   en la misma versión. La fusión ([`sync-upstream.sh`](../.github/ia/sync-upstream.sh))
   resuelve sola los choques que se repiten en cada fusión (`Cargo.lock`, los documentos del
   original, `ATTRIBUTION.md`, `README.md`). El proyecto original reescribe su rama `release` en
   cada versión, así que la fusión no usa la base que elegiría git, sino la última versión del
   original que ya se fusionó (guardada en `.github/ia/upstream.rev`): así solo pueden chocar
   las líneas que este fork cambió de verdad, y no el número de versión ni decenas de archivos
   en cada versión nueva. Si el proyecto original cambia las mismas líneas de
   **código** que este fork, Claude los resuelve (si existe el secreto `CLAUDE_CODE_OAUTH_TOKEN`)
   y su arreglo solo se sube si pasan las mismas pruebas. Lo mismo si la fusión no choca pero
   el asistente deja de compilar porque el original renombró algo que usa: Claude adapta el
   código del asistente. Nada que falle llega a `main`, y la
   última versión sigue disponible.
3. **Toca lo mínimo del código original**, para que esas fusiones casi nunca choquen:
   - todo el asistente vive en `apps/gridcraft-assistant` (compartido con Android);
   - la app de escritorio solo añade `apps/gridcraft/src/assistant_host.rs` y cuatro llamadas
     en `main.rs`, y la cinta unas pocas líneas.
4. **No instala las actualizaciones del original.** GridCraft trae su propio actualizador, que
   instalaría el GridCraft oficial encima (sin asistente). En GridCraft IA esa comprobación al
   abrir está apagada, y Ayuda › Buscar actualizaciones abre la del asistente, que mira las
   versiones de este fork.

### Si la sincronización se detiene, se nota

Una sincronización atascada nunca queda en silencio:

- **En la app:** si la versión más reciente con el archivo de tu plataforma (zip de Linux o de
  Windows, APK de Android) tiene más de 7 días, aparece «Las actualizaciones se detuvieron» con
  la fecha y dónde mirar. También queda escrito en asistente › engranaje › Actualizaciones.
  Funciona aunque GitHub no avise de nada.
- **En GitHub:** la ejecución termina en rojo (GitHub manda un correo cuando falla un workflow
  programado), el resumen de la ejecución explica qué pasó y qué hacer, y se abre un issue si
  el repositorio tiene los issues activados (*Settings › General › Features › Issues*).
- **El vigilante:** cada día, el paso «Watchdog» del workflow falla si la versión más reciente
  tiene más de 3 días, y avisa si le falta la APK o el zip de Linux.

### Aviso de versión nueva

Las apps (Windows, Linux y Android) consultan dos veces al día la lista pública de versiones del
repositorio en GitHub. No se envía nada del usuario: es la misma lista que se ve en *Releases*.
Si hay una compilación más nueva que la instalada, aparece el aviso «Hay una versión nueva» con
**Actualizar** y **Ahora no** (no vuelve a avisar de esa compilación). En el asistente ›
engranaje › **Actualizaciones** se puede desactivar el aviso o buscar al momento.

**Archivo › Actualizar.** El menú de la pestaña Archivo (Inicio, Nuevo, Abrir, Información…)
tiene siempre una fila para esto. Si la app ya encontró una versión nueva, dice «Actualizar a
0.17.0» con un punto naranja, y un clic la descarga, la instala y vuelve a abrir la app, aunque
antes se hubiera pulsado «Ahora no». Si no, dice «Buscar actualizaciones» y busca al momento.

- **Windows:** «Actualizar» descarga el zip de la versión y saca de él solo `gridcraft.exe`,
  `gridcraft-cli.exe` y `LEEME-IA.txt`. Un pequeño script de PowerShell, oculto, espera a que la
  app se cierre, copia esos archivos encima de esta copia y la vuelve a abrir. La app se cierra
  por su camino normal, así que pregunta antes si hay cambios sin guardar. Si se cancela el
  cierre, la versión nueva se instala al cerrar la app. Si no se pudo reemplazar el programa
  (por ejemplo, porque estaba en una carpeta sin permiso de escritura), la próxima vez que se
  abre la app lo dice en el aviso, junto con «Reintentar».
- **Linux:** «Actualizar» descarga el zip `-linux-x64.zip` y saca de él solo `gridcraft`,
  `gridcraft-cli` y `LEEME-IA.txt`. Linux deja reemplazar el archivo de un programa abierto, así que
  los pone en su sitio al momento (primero los copia al lado y luego los renombra, para que un
  fallo deje la copia anterior intacta). Después la app se cierra por su camino normal y un
  pequeño script de `sh` la vuelve a abrir cuando terminó. Si se cancela el cierre, la versión
  nueva se usa la próxima vez que se abra.
- **Android:** «Actualizar» descarga la APK dentro de la app y se la pasa al instalador de
  Android (`PackageInstaller`), que muestra su ventana: se pulsa «Actualizar» y listo. La primera
  vez Android pide activar «Permitir de esta fuente» para la app; al volver, la instalación sigue
  sola. Android solo acepta la APK si es la misma app firmada con la misma clave, y se conservan
  documentos, ajustes y claves. Tras instalar, se vuelve a abrir la app desde su icono.

Cada compilación del workflow lleva su número (`CRAFT_IA_BUILD`) y su repositorio
(`CRAFT_IA_REPO`); una compilación hecha a mano tiene el número 0, así que avisa de la primera
versión publicada. El código está en `apps/gridcraft-assistant/src/update.rs`.

### Ajustes del repositorio

- El workflow necesita que `main` contenga este trabajo: fusiona el pull request.
- Necesario: crea el secreto **SYNC_TOKEN**, un token personal con los permisos `repo` y
  `workflow`. Sin él, GitHub impide que el workflow suba una fusión en la que el proyecto
  original cambió sus propios workflows (lo hace a menudo), y la ejecución termina en rojo.
- Opcional: el secreto **CLAUDE_CODE_OAUTH_TOKEN** (en una terminal con Claude Code:
  `claude setup-token`) deja que Claude resuelva los choques de código. Usa tu plan de Claude.
- Para la APK de Android: crea los secretos **ANDROID_KEYSTORE_BASE64** (el archivo
  `craft-release.p12` en base64) y **ANDROID_KEYSTORE_PASSWORD** (su contraseña). Tiene que ser la
  misma clave con la que se firmaron las APK que ya tienes instaladas; si no, Android no deja
  instalar encima. Sin estos secretos, la versión solo trae los zip de Windows y Linux.
- Para publicar una versión al momento: *Actions* › «IA Windows» › *Run workflow*.

## Ubuntu (Linux)

Cada versión trae `GridCraft-IA-<versión>-linux-x64.zip`, compilado en Ubuntu 22.04, así que funciona
en 22.04 y en todas las posteriores. Para instalarlo:

1. Descomprime el zip (clic derecho › «Extraer aquí»).
2. En la carpeta «GridCraft IA», clic derecho › «Abrir en una terminal» y ejecuta `sh install-ia.sh`.

El script copia el programa a `~/.local/opt/gridcraft-ia` y añade «GridCraft IA» al menú de
aplicaciones (y al escritorio, si lo hay), con su icono y los tipos de archivo para «Abrir con».
También deja el comando `gridcraft-ia` en `~/.local/bin`. No necesita `sudo`, y un GridCraft oficial
(deb, AppImage, Flatpak) puede convivir con él.

- **Claves de API:** en Linux se guardan en `~/.config/gridcraft/assistant-keys.json`, con permisos
  `600` (solo tu usuario puede leerlas).
- **Plan de ChatGPT / Claude:** «Instalar» e «Iniciar sesión» abren una terminal (GNOME Terminal,
  Ptyxis, Konsole, xterm…). Claude Code se instala con su instalador oficial (`curl` o `wget`).
  Codex se instala con `npm` en `~/.local`, sin `sudo`; si falta npm, la terminal indica
  `sudo apt install nodejs npm`. La app busca ambos programas en `~/.local/bin` aunque no estén en
  el `PATH` del menú de aplicaciones.

## Desarrollo

- Pruebas: `cargo test -p gridcraft-assistant --lib`. Incluyen una prueba de extremo a extremo
  de la vía con plan, con una CLI falsa que edita el documento a través del puente.
- Vista previa del panel sin ventana:
  `cargo run -p gridcraft-assistant --example assistant_preview -- <carpeta> desktop-plans desktop-chat`.
- Compilación para Linux: `cargo build --release -p gridcraft -p gridcraft-cli`, y empaquetar como el
  job `linux` del workflow.
- Compilación cruzada desde Linux:
  `cargo build --release -p gridcraft -p gridcraft-cli --target x86_64-pc-windows-gnu`.
  Necesita MinGW. Las versiones publicadas usan MSVC en `windows-latest`.
