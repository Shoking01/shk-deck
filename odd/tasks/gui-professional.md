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
| T1 | `ss550-tray::install::refresh_installed` + arranque que sincroniza la copia instalada | S1 | inline | hecho — `29c6bde` |
| T2 | Scripts de actualización fiables (detectan fallo de build, verifican el hash, relanzan) | S1 | inline | hecho — `29c6bde` |
| T3 | Carcasa: barra superior, raíl, barra de estado, `Page` | S2, S5 | delegado | hecho — `0efdded` |
| T4 | Página `Deck` + inspector de tecla seleccionada + teclas con imagen | S3, S4, S6 | delegado | hecho — `0efdded` |
| T5 | Páginas `Actions`, `Artwork`, `Settings` | S3, S6 | delegado | hecho — `0efdded` |
| T6 | Verificación visual con captura de pantalla y corrección | S5, S7 | inline | hecho — `0efdded` |

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

**L3** (2026-11-14): carcasa, páginas y verificación con capturas reales.
- El deck conectado (15 teclas, 5 columnas) permitió juzgar la ventana de verdad:
  la primera captura mostró pasos numerados, columna izquierda vacía, contenido
  cortado abajo y un nombre de asset desbordando el panel. La segunda y la
  tercera corrigieron eso: raíl con cuatro páginas, contenido con scroll, cadenas
  truncadas dentro de un padre `min_w(0)`, teclas que dibujan su imagen con la
  vista previa cacheada, inspector con la imagen de la tecla seleccionada.
- La captura se tomó con `PrintWindow`, no con el escritorio: el escritorio
  estaba ocupado y una captura de pantalla habría fotografiado otra cosa.
- Dos defectos que solo se ven en pantalla: las cuerdas de estado salían con
  mojibake (`â€"` en vez de `—`) porque el literal del fuente se había
  corrompido en un viaje de ida y vuelta por una página de códigos, y el
  interruptor de inicio usaba el `primary` del tema (casi blanco en oscuro) en
  vez del acento. Ambos corregidos.
- La nota "la copia del inicio de sesión coincide" solo aparece en el proceso que
  realmente reemplaza la copia; en el siguiente arranque no hay nada que decir.
  Eso hizo que una captura pareciera un fallo cuando era el comportamiento
  buscado.
- Suites: 63 pruebas del GUI, 17 + 1 + 19 del tray, `cargo test --workspace`
  entero en verde. Commits `82e9f39`, `29c6bde`, `0efdded`.

**L4** (2026-11-15, reporte del usuario): "la imagen sale descuadrada, con un
borde a la izquierda y arriba, y no está en máxima calidad".

**Diagnóstico, con números.** Escala de grises media de las primeras filas y
columnas del asset `012-SOFTWARE_brave.jpg` (144×144) antes de tocarlo:

| Borde | Media | Vecinas | Veredicto |
| --- | --- | --- | --- |
| fila 0 | 45.9 | 29.7 | línea clara de 1 px horneada en el archivo |
| columna 0 | 28.6 | 11.6 | ídem |
| fila 143 | 28.5 | 11.2 | ídem |

No era el pipeline: `fit()` centra y rellena de negro, y el deck no añade marco.
El archivo traía un marco de 1 px de un recorte anterior. Al escalar de 144 a 100
esa línea se mezcla con la primera fila de salida y se hace más visible, no menos.

**Calidad.** El límite real era la escalera, que empezaba en 80 por coherencia
con los colores planos. Un tile de 100×100 a 95 ocupa unos 4 KB de los 9 KB
permitidos: el deck tenía sitio de sobra y la app no lo usaba. La escalera pasa a
`[95, 88, 80, 65, 50, 35, 20]`, con un test que compara el tile contra una
codificación hecha en el peldaño superior.

**Datos corregidos a mano, con copia de seguridad:** el asset se recortó 1 px por
lado (142×142, JPEG q100; original en `012-SOFTWARE_brave.jpg.bak`). Tras el
recorte la fila 0 mide 29.7 y la columna 0 mide 11.4: ya son el fondo de la
imagen, no una línea. Esto es un arreglo de datos, no de código: otro archivo con
marco volvería a mostrarlo, porque el render lo respeta tal cual.

- Commits: `73a1d70` (calidad) — pendiente de subir, como el resto.
