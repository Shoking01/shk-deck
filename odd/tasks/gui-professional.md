# GUI profesional con GPUI Kit

**Objetivo:** que la ventana se vea como una aplicación de escritorio terminada
—no como un panel de depuración con pasos numerados— y que al arrancar el
controlador sea siempre la build que el usuario acaba de compilar.
**Rama:** `feature/phase4-assets`. **Runner:** `cargo test` en
`crates/ss550-gui` + `cargo test --manifest-path crates/ss550-tray/Cargo.toml` +
`cargo test --workspace` en la raíz.

## Specs

- **S1. La build instalada es la que corre.** La entrada de inicio de sesión
  apunta a `%LOCALAPPDATA%\Programs\ss550\ss550-gui.exe`. Al arrancar, la
  aplicación compara esa copia con el binario en ejecución; si son bytes
  distintos, la reemplaza y lo dice en la página de ajustes. Una build de
  depuración no sobrescribe la copia instalada. Si la copia no se puede
  reemplazar (por ejemplo, porque la instancia vieja sigue abierta), la ventana
  lo dice con la ruta y el motivo, en vez de callarse.
- **S2. Una sola carcasa.** Barra superior (nombre del producto, deck activo,
  estado de conexión, tema, perfil); raíl de navegación a la izquierda; área de
  contenido con scroll; barra de estado abajo. La ventana nunca muestra los
  pasos "STEP 1…5": la navegación reemplaza esa secuencia.
- **S3. Cuatro páginas.** `Deck` (rejilla y controles de color/brillo),
  `Actions` (asignar acciones a teclas), `Artwork` (biblioteca de imágenes y
  vista previa) y `Settings` (inicio con Windows, carpetas, tema, diagnóstico).
  Cambiar de página no desconecta ni pierde estado.
- **S4. El deck manda.** Filas, columnas, número de teclas, tamaño de imagen y
  controles disponibles salen del `DeckSession`. No se escribe `15`, ni un
  modelo, ni un tamaño de píxel en el GUI.
- **S5. Lenguaje visual único.** Tokens fijos: fondo `#0b0b0f`, superficie
  `#141419`, superficie elevada `#1b1b22`, borde `#26262f`, texto `#ededf0`,
  texto secundario `#a1a1ad`, acento `#7c6cf0`. Radios: 8 (controles), 12
  (tarjetas), 999 (píldoras). Espaciado en múltiplos de 4. Tipografía: 20/15/13
  px, micro-etiquetas de sección en 11 px mayúsculas con tracking. Un solo
  acento; el blanco y el rojo de las teclas son datos, no cromo.
- **S6. Estados, no textos muertos.** Toda pantalla tiene estado vacío, de
  error y de éxito explícitos: sin deck conectado se explica qué hacer; una
  acción rechazada nombra el motivo; una acción aceptada se confirma en la barra
  de estado.
- **S7. Accesibilidad y teclado.** Todo control es alcanzable con Tab y
  activable con Enter/Espacio; los iconos llevan texto o `tooltip`; el contraste
  de texto sobre superficie cumple AA (≥ 4.5:1).
- **S8. Rendimiento.** El primer frame sigue por debajo del presupuesto de
  `docs/milestones/M5-performance-budgets.md`; ninguna página decodifica una
  imagen durante `render` (las vistas previas se construyen al cambiar la
  selección, no en cada frame).

## Tasks

| ID | Qué | Specs | Ruta | Estado |
| --- | --- | --- | --- | --- |
| T1 | `ss550-tray::install::refresh_installed` + arranque que sincroniza la copia instalada | S1 | inline | hecho — pendiente de commit |
| T2 | Scripts de actualización fiables (detectan fallo de build, verifican el hash, relanzan) | S1 | inline | hecho — pendiente de commit |
| T3 | Carcasa: barra superior, raíl, barra de estado, `Page` | S2, S5 | delegado | pendiente |
| T4 | Página `Deck` + inspector de tecla seleccionada | S3, S4, S6 | delegado | pendiente |
| T5 | Páginas `Actions`, `Artwork`, `Settings` | S3, S6 | delegado | pendiente |
| T6 | Verificación visual con captura de pantalla y corrección | S5, S7 | inline | pendiente |

## Log

**L1** (2026-11-14, petición del usuario, verbatim):

> Al iniciar la app sigue abriendose con la build anterior. Necesitamos solventar
> esto asi como mejorar la GUI al maximo con GPUI y llevarla a verse como una app
> profesional

**Análisis:** la entrada `HKCU\…\Run\ss550-gui` apunta a
`%LOCALAPPDATA%\Programs\ss550\ss550-gui.exe`, una copia del 13/11 00:17 que solo
se vuelve a escribir cuando el usuario enciende el interruptor "START WITH
WINDOWS". Todo lo compilado después (rotación 180°, tiles 100×100, picker) vivía
en `crates/ss550-gui/target/` y nunca llegó a esa ruta: de ahí "sigue abriéndose
la build anterior". La copia instalada se reemplazó a mano al diagnosticar, y la
aplicación ahora la sincroniza sola en cada arranque (S1).

**L2** (2026-11-14): el test `the_window_never_names_the_deck_it_drives` fallaba
por los comentarios nuevos de `render.rs` (nombraban el modelo). Corregidos para
hablar de "the deck"; el test vuelve a verde con 46 pruebas en el GUI y 17 en
`ss550-tray`.
