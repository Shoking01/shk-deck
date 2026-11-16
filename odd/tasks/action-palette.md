# Paleta de acciones y teclas que corren cosas

**Objetivo:** que la ventana se use como un Stream Deck profesional: un panel
lateral con acciones arrastrables que se sueltan sobre una tecla, y una tecla
que muestra sola la imagen de lo que ejecuta.
**Rama:** `feature/phase4-assets`. **Runner:** `cargo test --workspace` +
`cargo test` en `crates/ss550-gui` + `cargo test --manifest-path
crates/ss550-tray/Cargo.toml`.

## Specs

- **S1. Tocar una tecla no la modifica.** Un clic selecciona; nunca pinta,
  nunca asigna, nunca borra. Pintar es un gesto explícito desde la tarjeta de la
  tecla seleccionada. *(hecho)*
- **S2. El panel lateral es una paleta.** Además de la navegación, ofrece las
  acciones como entradas arrastrables: `Run app`, `Open URL`, `Keystrokes`, y
  cada acción que ya exista en el deck.
- **S3. Arrastrar y soltar asigna.** Soltar una entrada de la paleta sobre una
  tecla la convierte en la acción de esa tecla. Si la acción necesita
  parámetros, se abre su editor con el primer campo enfocado y la tecla no
  cambia hasta guardar; cancelar deja la tecla como estaba.
- **S4. Acción `keys`.** Envía una combinación de modificadores y una tecla
  (por ejemplo `Ctrl+Shift+M`) a la ventana que tiene el foco. Se valida al
  construirla: una combinación vacía, dos teclas base o un modificador
  desconocido se rechazan con un error tipado que nombra el problema.
- **S5. La tecla muestra lo que ejecuta.** Si la acción es `run app`, la tecla
  dibuja el icono de ese ejecutable, extraído del propio archivo; si no hay
  icono legible, un glifo con la inicial y el acento del sistema. Lo que la
  ventana muestra y lo que se envía a la tecla es la misma imagen.
- **S6. Artwork deja de ser una página.** La imagen personalizada pasa a ser una
  opción dentro del editor de la tecla; la biblioteca sigue existiendo en
  `config_dir()/assets/`.
- **S7. Sin decodificar en `render`.** Los iconos y las vistas previas se
  preparan cuando cambia la acción, la biblioteca o el perfil.
- **S8. `unsafe` solo en `ss550-tray`.** La extracción de iconos y el envío de
  pulsaciones son Win32 y viven ahí, con su excepción declarada; el resto del
  proyecto sigue con `#![deny(unsafe_code)]`.
- **S9. Sin deck conectado todo sigue siendo navegable.** Se puede armar la
  paleta, editar teclas y ver qué haría cada una; lo único que no ocurre es el
  efecto en el hardware.

## Tasks

| ID | Qué | Specs | Ruta | Estado |
| --- | --- | --- | --- | --- |
| T1 | Clic en la tecla = seleccionar; colores pasan a la tarjeta de la tecla | S1 | inline | hecho |
| T2 | `Action::Keys` en `ss550-engine`: modelo, validación, resumen, JSON | S4 | delegado | hecho — pendiente de commit |
| T3 | Envío de pulsaciones (Win32 `SendInput`) en `ss550-tray` | S4, S8 | delegado | hecho — `f2845b0` |
| T4 | Paleta lateral arrastrable + soltar sobre tecla + editor por tecla | S2, S3 | delegado | hecho — `f36ef07` |
| T5 | Icono del ejecutable como imagen de la tecla | S5, S7 | delegado | hecho |
| T6 | Artwork dentro del editor de la tecla | S6 | delegado | pendiente — el editor ya vive en la columna del `Deck`; falta mover ahí la imagen personalizada |

## Log

**L1** (2026-11-15, petición del usuario, verbatim):

> Perfecto. Ahora el tema de pintar las teclas esto viene del testing y era util
> para eso pero me gustaria ocultarlo o quitarlo ya que hay keys que van a
> necesitar parametros y si cada vez que las toco las pinto no va a funcionar
> bien. Tambien el apartado de Artwork es algo arcaico. Estaria bien que si
> selecciono que ejecute una app se ponga la imagen de esta. En si el objetivo es
> tener un menu lateral y que yo arrastre una opción que quiero que haga cada
> tecla. Ejecutar una app, abrir una URL, ejecutar una tecla o una combinación de
> teclas. Asi va formandose mas a una app de Stream Deck profesional

**L2** (2026-11-15): T1 entregada. El gesto de pintar salió del tile y del
toolbar; la tarjeta de la tecla seleccionada es ahora el único sitio donde se
elige color, y solo cambia esa tecla. El subtítulo de la página dejó de decir
"click a key to paint it".

**L3** (2026-11-15): T2 entregada. `Action::Keys { keys: KeyCombo }` en el
motor: `KeyCombo` solo se construye por `parse`/`FromStr`/`Deserialize`, así que
una combinación inválida no puede existir; el JSON guarda `"ctrl+shift+m"`; el
runner recibe el emisor por inyección (`KeySender`) y sin emisor devuelve
`NeedsKeySender` de forma síncrona, nunca un éxito que no ocurrió. 385 pruebas
del workspace en verde; el GUI queda roto en un único `match` hasta la unidad
siguiente.

**Nota de diagnóstico (2026-11-15):** el borde de arriba y la izquierda del que
se venía hablando no era centrado ni codificado: el panel mide 108 px, no los
100 que envía la referencia de la comunidad, así que un payload de 100 dejaba 8
px sin pintar. El payload pasó a 108×108 y el borde desapareció. En el mismo
paso se encontró que los perfiles se guardaban en `profiles/<deck key>.json` con
la clave cruda, que contiene `:` — en Windows eso es un *alternate data stream*,
no un nombre de archivo: el perfil vivía dentro del stream de un archivo llamado
`shape`. Ahora se guarda con slug + huella, y la clave del deck ya no incluye el
tamaño de imagen.

**L4** (2026-11-15): T5 entregada, y con ella la mitad de T6 que era deuda de T4.
Un `run app` cuyo perfil no trae imagen propia dibuja ahora el icono del
programa: `ss550-tray` se lo pide a la shell (`SHGetFileInfoW` con
`SHGFI_ICON | SHGFI_LARGEICON`), así que un `.lnk` da el icono de su destino y no
el de una hoja en blanco, y el `HICON` se destruye antes de volver. La extracción
ocurre una vez por ruta, cuando cambia la acción, el perfil o el deck, nunca en
`render`; un rebind olvida el icono que ya no se usa. El tile y la tecla salen
del mismo `render`: el tile pinta el frame ajustado y la tecla recibe el JPEG del
mismo ajuste y la misma escalera de calidad. Un programa sin icono legible, una
URL y una combinación caen a la inicial sobre el acento, dibujada como bitmap
5×7 al tamaño de imagen del deck, así que la tecla también recibe imagen; la fila
de color sigue siendo la excepción explícita. El editor de la tecla se mudó a la
columna del `Deck` — soltar una plantilla ya no manda la ventana a `Actions`:
abre el editor ahí mismo, con el primer campo enfocado — y `Actions` quedó como
el resumen (chips de teclas, el mismo editor, la lista de lo que está atado),
porque ahora los dos sitios dibujan un solo widget. Queda para T6 llevar la
imagen personalizada dentro de ese editor y retirar la página.

