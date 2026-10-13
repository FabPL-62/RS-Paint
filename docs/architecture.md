# Arquitectura de edición

La dependencia va en una sola dirección:

```text
rspaint-app (egui, ventanas y sistema operativo)
    ├── rspaint-editor (estado y operaciones de edición)
    │       └── rspaint-core (algoritmos y buffers de imagen)
    └── rspaint-core (codificar y decodificar archivos)
```

## Componente de dibujo

`rspaint-editor::Editor` posee el documento, historial, selecciones y operaciones
en curso. `DocumentColors` conserva por documento los colores primario y
secundario, la paleta tradicional y los colores capturados. `ToolSettings`
agrupa las opciones compartidas entre pestañas: tamaños, formas, relleno,
mezcla, texto y tolerancia. Los snapshots de trazos,
curvas, texto y transformaciones son privados al editor.

La captura y ordenación por tono viven en `rspaint-core`; el editor conserva
el resultado completo. La interfaz solamente decide qué dos filas mostrar y
mantiene la posición de desplazamiento de cada pestaña.

La UI selecciona herramientas mediante `set_tool`, modifica las opciones y
traduce eventos a métodos como `begin_stroke`, `update_stroke`, `end_stroke`,
`begin_selection_drag`, `resize_document`, `flip_rotate` y `stretch_skew`.
Cambiar de herramienta permite al editor confirmar o cancelar sus operaciones.
No debe implementarse un algoritmo de edición en un callback de egui.

El editor depende de `emath` para puntos y vectores, de `ab_glyph` para rasterizar
texto y de `epaint_default_fonts` como datos de fuentes de respaldo. No depende
de egui, eframe, wgpu, rfd ni arboard. Puede crearse con `Editor::new()` y usarse
sin ventana ni GPU. Las fuentes del sistema se cargan al construirlo.

Los módulos separan trazos y formas, texto, selección, pincel de selección,
transformación de selección, transformaciones de imagen y navegación. Los
helpers internos se comparten solo dentro del crate.

## Presentación y adaptadores

`PaintApp` compone un `Editor` con estado de presentación: diálogos, texturas,
cursores y opciones temporales de formularios. No utiliza `Deref` para esconder
la separación; las llamadas al editor son explícitas.

- `app/input.rs`: atajos y traducción de modificadores.
- `app/files.rs`: archivos, diálogos nativos y portapapeles del sistema.
- `app/dialogs.rs`: presentación de formularios y vistas auxiliares.
- `app/presentation.rs`: composición de paneles y datos del shader.
- `ui/canvas_view.rs`: composición del viewport; submódulos para entrada,
  geometría de controles y overlays.
- `ui/tabs.rs`: apariencia de pestañas; `workspace.rs` conserva sus acciones,
  documentos y ventanas.

Al abrir un archivo, el adaptador decodifica la imagen y emite una petición de
documento nuevo. El workspace crea y activa otra pestaña en la misma ventana;
la pestaña anterior conserva sus píxeles, selección, historial y cambios pendientes.
La pestaña temporal de inicio se reutiliza si aún es un documento blanco intacto.
La misma regla se aplica a hojas nuevas intactas y a archivos arrastrados sobre
la ventana. El diálogo y drag and drop comparten la lectura y decodificación;
los drops se consumen una sola vez y cada imagen se procesa por separado.
Un archivo abierto o una hoja con historial de operaciones nunca se descarta,
incluso cuando deshacer deja sus píxeles blancos de nuevo.

`take_pending_upload()` entrega bytes RGBA completos o de regiones cambiadas;
`canvas_renderer` es el adaptador wgpu que los sube a la textura. `revision()`
permite invalidar la miniatura sin guardar texturas ni flags de widgets en el
editor. El portapapeles interno pertenece al editor; el adaptador le proporciona
una imagen externa cuando existe. Los diálogos pasan valores a operaciones del
editor, que no conocen la visibilidad ni el layout del formulario.

## Idiomas y recursos

Los mensajes se centralizan en `crates/rspaint-app/locales/es.json` y `en.json`.
La clave es el texto canónico en inglés. `i18n::tr` resuelve una clave;
`i18n::message` sustituye parámetros con nombre como `{name}` o `{error}`.
El historial conserva claves canónicas y se traduce al mostrarse.

El menú **Idioma / Language** cambia el idioma para todas las ventanas durante
la sesión. El predeterminado es español. Los nombres de archivos y documentos
del usuario no se traducen. Para añadir un idioma, copia un catálogo, traduce
sus valores y registra la variante en `i18n::Locale`, incluyendo `ALL`, `name`
y el catálogo en `text`. Los tests verifican claves y parámetros de ambos idiomas.

Los 20 iconos se incrustan desde `assets/tools`, se cargan una vez por contexto
y se muestrean con vecino más cercano. `hover.png` es un recurso decorativo y no
una herramienta adicional. La grilla conserva dos columnas; la barra lateral
permite desplazarse verticalmente en ventanas pequeñas.

## Verificación

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

`rspaint-editor/tests/editor.rs` prueba operaciones completas: trazos, recorte
por selección, preview cancelable, copiar/cortar/pegar, transformaciones,
historial, cambio de documento y actualizaciones parciales del renderer.
Los tests de UI comprueban catálogos, caché de iconos y la grilla renderizada.

La prueba del portapapeles nativo es manual porque necesita una sesión gráfica
y sobrescribe el contenido del portapapeles. Ejecutarla explícitamente:

```sh
cargo test -p rspaint-app clipboard_round_trip -- --ignored
```
