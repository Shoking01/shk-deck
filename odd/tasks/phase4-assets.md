# Fase 4 — el pipeline de imágenes

**Objetivo:** que una tecla pueda mostrar una imagen, no solo un color plano, y
que el modelo tenga hueco para imágenes generadas (reloj) sin reescribirse.
**Rama:** `feature/phase4-assets`. **Runner:** `cargo test --workspace` +
`cargo test --manifest-path crates/ss550-gui/Cargo.toml`.

## Specs

- **S1.** Una tecla muestra exactamente una cosa: un color plano o una imagen.
  El modelo es un mapa `tecla → Appearance`, no dos mapas que puedan
  contradecirse.
- **S2.** La imagen vive en una biblioteca que la app posee, en
  `config_dir()/assets/`. Un perfil guarda un **nombre de archivo**, nunca una
  ruta, para que mover la carpeta de configuración no deje un perfil apuntando a
  la nada.
- **S3.** Una imagen que no coincide con el `ImageSpec` de la deck se **ajusta**,
  no se estira: se conserva la proporción, se centra sobre negro y se codifica
  al ancho y alto que la deck declara. Ningún `100x100` en el código del GUI.
- **S4.** Si la imagen codificada supera `ImageSpec::max_bytes()`, se recodifica
  a menor calidad antes de rechazarla. El rechazo nombra el tamaño obtenido y el
  máximo aceptado; nunca se envía un payload que la deck va a rechazar.
- **S5.** Un perfil con arte sobrevive a un reinicio. Un archivo de perfil viejo
  que solo tiene `colors` se lee igual y se migra a `appearance` al cargar.
- **S6.** Un nombre de asset se valida al construirse: sin separadores, sin `..`,
  sin rutas absolutas, sin caracteres de control, con una extensión aceptada. Un
  nombre inválido no puede existir en memoria, así que no hay ruta que un
  archivo de configuración pueda inyectar.
- **S7.** La ventana lista la biblioteca, muestra una vista previa, asigna la
  imagen a la tecla seleccionada y ofrece abrir la carpeta. Sin diálogo nativo
  en esta entrega: se arrastran archivos a la carpeta y la ventana los lista.
- **S8.** Fuera de esta entrega, registrado para no perderlo:
  - **Combinaciones de teclas** = una variante nueva de `Action` en
    `ss550-engine`, no una apariencia.
  - **Reloj digital** = una variante `Generated` de `Appearance` más un tick de
    repintado en el GUI.
  - **Plugins de terceros** = WASM sobre las dos capas anteriores.

## Tasks

| ID | Qué | Specs | Ruta | Estado |
| --- | --- | --- | --- | --- |
| T1 | `ss550-engine::assets` — `AssetName` validado, `AssetLibrary` (listar, importar, resolver) | S2, S6 | inline | hecho — `d50f404` |
| T2 | `ss550-engine::appearance` — `Appearance`, campo en `Profile`, migración desde `colors` | S1, S5 | inline | hecho — `c592cd4` |
| T3 | GUI — pipeline de render: decodificar, ajustar, escalera de calidad, caché | S3, S4 | inline | hecho — `4be2ed9` |
| T4 | GUI — selector de imagen por tecla, vista previa, abrir carpeta, refrescar biblioteca | S7 | inline | hecho — `e73bcd7` |
| T5 | Docs — README, CHANGELOG, PLAN | S1-S8 | inline | hecho — `fb5dd82` |

## Log

**L1** (2026-11-14, verbatim): «Vamos con Fase 4. La idea de plugins puede
variar demasiado, quiero plugins para controlar apps. Ejecutar combinaciones de
teclas, reloj digital y mas cosas.»

**Análisis:** lo pedido son tres capas distintas y S8 las separa. Fase 4 construye
la capa de apariencia; las combinaciones de teclas son una acción y el reloj es
una apariencia generada. El modelo se elige para que `Generated` sea una variante
nueva y no una migración.

**Decisión:** una sola fuente de verdad por tecla (`appearance`), y `colors`
queda como campo heredado que solo se lee para migrar. Dos mapas para el mismo
concepto envejecen mal y el reloj los habría obligado a converger igual.

**L2** (2026-11-14): T1 y T2 entregadas. T1 `d50f404` (69 tests de motor),
T2 `c592cd4` (79 tests de motor). La migración se probó con tres archivos:
uno solo con `colors`, uno mixto donde la apariencia explícita gana, y un
ciclo guardar/cargar con una imagen.

**L3** (tarea incompleta): T4 está casi terminada. Implementado:
- `refresh_library()` y `selected_asset`/`tiles`/`library` en `Controller`
- `select_asset()`, `assign_asset()`, `remove_artwork()`, `save_profile()`,
  `repaint_key()`, `open_assets_folder()`
- Panel ARTWORK con lista de imágenes, vista previa, botones USE/REMOVE/OPEN/REFRESH
- Test para `set_appearance` (ya existía), tests nuevos para render

Falta: ajustar el número de líneas del commit T4 para que no sea demasiado grande.

**L4** (2026-11-14): T4 y T5 entregadas. Todos los tests pasan (46 GUI + 80 motor).
Commits:
- `e73bcd7` — Picker y `set_appearance()`
- `fb5dd82` — Docs actualizados (README, CHANGELOG)
