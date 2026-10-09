# Plan de desarrollo: RS Paint (port de JS Paint a Rust)

## 1. Resumen ejecutivo

`jspaint/` es una recreación de MS Paint escrita en JavaScript vanilla sobre Canvas 2D
(~45 módulos en `src/`, ~600 KB de código, más librerías en `lib/`). `rspaint/` es el
espacio destinado a su versión nativa en Rust.

**Objetivo:** construir una app de escritorio nativa en Rust con `egui` + `wgpu` que
alcance un **MVP funcional** (lienzo, herramientas de dibujo esenciales, paleta,
undo/redo y guardar/abrir PNG/BMP) y, después, avanzar por fases hacia paridad de
comportamiento con JS Paint.

Este documento es el plan maestro. Cada fase produce software ejecutable y verificable.

### Estado actual (actualizado)

- **Fase 0:** completa.
- **Fase 1 (MVP):** completa.
- **Fase 2:** en curso — hechas Attributes/Clear, Flip/Rotate, Stretch/Skew, Invert,
  Crop to Selection, Polygon; falta Curve, toggle de grilla, Zoom Custom/To Window,
  View Bitmap, Thumbnail, quick undo.
- **Extras ya implementados (más allá del plan):** render del canvas con pase wgpu
  propio (checkerboard, grilla y cursor en el shader), pan con botón central,
  zoom Ctrl+rueda anclado al cursor, varita mágica, 3 estilos de relleno de formas,
  brochas diagonales, restricción Shift (cuadrado/círculo y líneas a 45°), radio de
  esquinas ajustable, transparencia de selección con Ctrl, Color Eraser.
- **Verificación:** `cargo fmt`, `cargo clippy -D warnings` y `cargo test` en verde
  (~78 tests) + smoke test de arranque. El *parity harness* (sección 8) aún no está
  implementado.

---

## 2. Decisiones tomadas

| Tema              | Decisión                                                        |
|-------------------|-----------------------------------------------------------------|
| Plataforma        | App de escritorio nativa en Rust                                |
| UI                | `egui` (widgets, ventanas, menús) sobre `eframe`                |
| Render de GPU     | `wgpu` vía `egui-wgpu` (para zoom/pan fluido en imágenes grandes)|
| Raster del lienzo | Software sobre `Vec<u8>` RGBA premultiplicado, subido a textura  |
| Alcance inicial   | MVP funcional; resto de features por fases                      |
| Licencia          | MIT, consistente con jspaint                                    |

### Principio rector: núcleo separado de la UI

Todo el comportamiento del editor vive en un crate `rspaint-core` **sin dependencias
de GUI**. Esto permite:

- Tests unitarios y de snapshot sin abrir ventanas.
- Reutilizar el núcleo en una futura versión WASM o CLI.
- Portar con lógica, no con "código de UI con lógica incrustada" (que es como está
  hoy jspaint: `app.js` mezcla estado global, DOM y canvas).

---

## 3. Stack de crates propuesto

| Necesidad                         | Crate                                            | Notas |
|-----------------------------------|--------------------------------------------------|-------|
| Ventana + UI immediata            | `eframe`, `egui`, `egui_extras`                  | Look retro con estilo custom |
| GPU / texturas                    | `wgpu`, `egui-wgpu`                              | Integrado por eframe |
| Raster de alto nivel (paths, AA)  | `tiny-skia`                                      | Alternativa/complemento al raster manual |
| Primitivas de imagen              | `imageproc`                                      | Flood fill, draws simples |
| Decodificación/encodificación     | `image` (PNG, JPEG, GIF, BMP, TIFF, WebP)        | Base de formatos |
| Texto rasterizado (tool Text)     | `cosmic-text` + `swash` (o `ab_glyph`)           | Para dibujar texto en el buffer |
| Diálogos nativos de archivo       | `rfd`                                            | Open/Save |
| Portapapeles                      | `arboard`                                        | Copy/Paste de imágenes |
| Persistencia de estado/backup     | `serde`, `serde_json`, `dirs` (o `directories`)  | Reemplaza `localStorage` |
| i18n                              | `fluent` (o `rust-i18n`)                         | Reemplaza `app-localization.js` |
| Export GIF                        | `gif`                                            | "Render History As GIF" |
| Paralelismo                       | `rayon`                                          | Fill/transforms en imágenes grandes |
| Errores                           | `thiserror` (core), `anyhow` (app)               | |
| Logging                           | `tracing` + `tracing-subscriber`                 | |
| Tests de snapshot                 | `insta`, diff de imágenes propio                 | Ver sección 8 |

> Fijar versiones con `cargo add` y bloquear con `Cargo.lock`. Preferir crates
> mantenidos; si `tiny-skia` no reproduce el antialiasing de Canvas 2D, se documenta
> la divergencia (ver sección 8).

---

## 4. Estructura del workspace

```
rspaint/
  Cargo.toml                 # workspace
  plans/
    main.md                  # este documento
  crates/
    rspaint-core/            # lógica pura, sin GUI
      src/
        lib.rs
        document.rs          # Document, PixelBuffer, layers, flags
        color.rs             # RGBA, paletas, HSV/HSL, patterns
        tools/               # motor de herramientas
          mod.rs
          pencil.rs
          brush.rs
          eraser.rs
          fill.rs
          pick_color.rs
          line.rs
          shapes.rs          # rect, ellipse, rounded rect
          select.rs          # rect + free-form
          airbrush.rs
          magnifier.rs
          text.rs            # (fase posterior)
        history.rs           # grafo no lineal de undo/redo
        transform.rs         # flip, rotate, stretch/skew, invert
        canvas.rs            # buffer, blend, contigüidad, bresenham, bezier
        io/
          mod.rs
          image_formats.rs   # png/bmp/tiff/jpeg/gif/webp
          palette_formats.rs # RIFF PAL, GIMP, ACO/ASE, etc. (fase)
        geometry.rs          # rects, puntos, polígonos, triangulación
        palette.rs           # default_palette, monochrome, custom, basic
    rspaint-app/             # binario eframe
      src/
        main.rs
        app.rs               # struct App, impl eframe::App
        state.rs             # estado de UI (equivalente a app-state.js)
        ui/
          canvas_view.rs     # pan/zoom, grid, thumbnail
          toolbox.rs         # $ToolBox equivalente
          colorbox.rs        # $ColorBox equivalente
          menubar.rs         # menus.js equivalente
          tool_options.rs    # tool-options.js equivalente
          dialogs.rs         # msgbox.js, attributes, resize, flip/rotate...
        theme.rs             # temas classic/dark/modern
        shortcuts.rs         # atajos de teclado
  assets/
    icons/                   # reutilizar sprites de jspaint (MIT)
```

Regla de dependencias: `rspaint-app` -> `rspaint-core`. Nunca al revés.

---

## 5. Mapeo jspaint -> rspaint

| jspaint (`jspaint/src/...`)        | rspaint (`crates/...`)                          |
|------------------------------------|-------------------------------------------------|
| `app-state.js`                     | `rspaint-app/src/state.rs` + `core::document`   |
| `app.js`                           | `rspaint-app/src/app.rs`                        |
| `tools.js`                         | `core/src/tools/*` + `app/src/ui/toolbox.rs`    |
| `tool-options.js`                  | `app/src/ui/tool_options.rs`                    |
| `image-manipulation.js`            | `core/src/canvas.rs`, `core/src/transform.rs`   |
| `functions.js`                     | repartido: `core::history`, `core::io`, dialogs |
| `color-data.js`                    | `core/src/palette.rs`                           |
| `$ColorBox.js` / `$FontBox.js`     | `app/src/ui/colorbox.rs`                        |
| `$ToolBox.js` / `$ToolWindow.js`   | `app/src/ui/toolbox.rs`                         |
| `menus.js`                         | `app/src/ui/menubar.rs`                         |
| `Handles.js` / `OnCanvasSelection.js` | `core/src/tools/select.rs`                   |
| `OnCanvasTextBox.js`               | `core/src/tools/text.rs`                        |
| `OnCanvasHelperLayer.js`           | capa de preview en `canvas_view.rs`             |
| `file-format-data.js`              | `core/src/io/image_formats.rs`                  |
| `storage.js` / `manage-storage.js` | `app/src/state.rs` (persistencia con `dirs`)    |
| `theme.js` + `styles/*.css`        | `app/src/theme.rs`                              |
| `app-localization.js`              | `fluent`                                        |
| `sessions.js` (multiplayer)        | Fase tardía / opcional                          |
| `speech-recognition.js`            | Fase tardía / opcional                          |
| `eye-gaze-mode.js`                 | Fase tardía / opcional                          |
| `electron-main.js` / preload       | reemplazado por `rfd` + `arboard`               |
| `lib/UPNG.js`, `UTIF.js`, `bmp.js` | crates `image`/`imageproc`                      |
| `lib/gif.js`                       | crate `gif`                                     |
| `lib/anypalette-0.6.0.js`          | `core/src/io/palette_formats.rs` (fase)         |

---

## 6. Modelo de datos del núcleo

### 6.1 Documento y buffer

- `PixelBuffer { width, height, data: Vec<u8> }` en RGBA8, **premultiplicado**
  internamente para composición correcta; exponer helpers no-premultiplicados.
- `Document { buffer, transparent: bool, background: Color, selection: Option<Selection>, textbox: Option<TextBox>, palette, file_meta }`.
- `Selection { pixels: PixelBuffer, x, y, mask: Option<Mask>, tool_transparent_mode: bool }`.
- Los patrones (patterns tipo `CanvasPattern`) se modelan como `Pattern { tile: PixelBuffer }`
  y se aplican con `draw_with_swatch` equivalente.

### 6.2 Historial no lineal

jspaint usa un **grafo** de estados (`parent`, `futures`), no una pila.

- `HistoryNode { id, parent: Option<NodeId>, futures: Vec<NodeId>, timestamp, soft, snapshot: Snapshot, name, icon }`.
- `Snapshot` = imagen completa (MVP) o delta por región (optimización posterior).
- `undo()` / `redo()` navegan por el grafo; `jump_to(node)` para la ventana History.
- `undoable(name, f)` = tomar snapshot -> ejecutar -> registrar nodo.

### 6.3 Motor de herramientas (state machine)

Cada herramienta implementa:

```rust
pub trait Tool {
    fn pointer_down(&mut self, ctx: &mut ToolCtx, p: Point);
    fn pointer_move(&mut self, ctx: &mut ToolCtx, p: Point);
    fn pointer_up(&mut self, ctx: &mut ToolCtx, p: Point);
    fn preview(&self, layer: &mut PixelBuffer); // helper layer
}
```

`ToolCtx` da acceso a: colores (fg/bg/ternary), tamaños, `undoable`, `selection`,
`magnification`, botones/teclas (shift, ctrl, botón derecho), y "cancelar a" nodo
(quick undo).

---

## 7. Fases

### Fase 0 - Andamiaje (base) — COMPLETA
- [x] Crear workspace `rspaint/` con crates `rspaint-core` y `rspaint-app`.
- [x] `eframe` "hello world" con ventana, menú y lienzo vacío.
- [x] CI (GitHub Actions): `cargo fmt`, `cargo clippy -D warnings`, `cargo test`.
- [x] Copiar assets de iconos desde `jspaint/images` (verificar licencia MIT).

### Fase 1 - MVP funcional — COMPLETA
- [x] `PixelBuffer` + `Document` + render del lienzo con zoom/pan (pase wgpu propio).
- [x] Checkerboard de transparencia, cursor de píxel, coordenadas en status bar.
- [x] Paleta por defecto (28 colores) + colores básicos + custom; foreground/background.
- [x] Herramientas de dibujo: Pencil, Brush, Eraser/Color Eraser, Airbrush, Fill (contiguo y no contiguo), Pick Color, Line, Rectangle, Ellipse, Rounded Rectangle.
- [x] Bresenham + stamps de pincel (circle/square/diagonales), soporte de tamaño.
- [x] Select (rect) y Free-Form Select: mover, eliminar, modo transparente/opaco.
- [x] Historial no lineal con Undo/Redo + ventana History.
- [x] Archivo: New, Open, Save, Save As; PNG y BMP (leer/escribir); diálogos con `rfd`.
- [x] Atajos: Ctrl+S/O, Ctrl+Z/Y, Delete/Escape, letras de herramienta, +/-.
- [x] Título de ventana con nombre de archivo y marca de "modificado".

**Criterio de salida MVP:** dibujar, seleccionar, deshacer/rehacer, y guardar/abrir PNG
sin pérdidas de calidad en un flujo completo.

### Fase 2 - Paridad de edición de imagen — EN CURSO
- [x] Image > Attributes (tamaño + transparente), Clear Image.
- [x] Flip/Rotate (incl. ángulo arbitrario), Stretch/Skew, Invert Colors.
- [x] "Crop to Selection"; [ ] aplicar transformaciones solo a la selección.
- [x] Polygon (clic para vértices, cierre, relleno).
- [ ] Curve (bezier con 2 puntos de control).
- [ ] Grid manual (Ctrl+G); hoy la grilla aparece sola a partir de 8×.
- [ ] Zoom Custom / Zoom To Window, View Bitmap, Thumbnail.
- [x] Draw Opaque, brush scaling con numpad (+/-); [ ] quick undo con segundo botón.

### Fase 3 - Texto y edición avanzada
- [ ] Text tool con `cosmic-text`, text toolbar (fuente, tamaño, negrita/cursiva/subrayado).
- [ ] Caja de texto en canvas, preview a cualquier zoom, commit undoable.
- [ ] Copy/Cut/Paste vía `arboard`; Paste From / Copy To.
- [ ] Undo no lineal completo con regiones (diferencias).

### Fase 4 - Paletas, formatos y theming
- [ ] Leer/escribir formatos de paleta (RIFF PAL, GIMP GPL, ACO/ASE, Paint.NET, etc.).
- [ ] Más formatos de imagen: TIFF (1 página), JPEG, GIF, WebP.
- [ ] Temas Classic Light/Dark, Modern, Winter, Occult (estilo egui).
- [ ] i18n con `fluent`; RTL si es viable.
- [ ] Persistencia de preferencias + backup automático por documento.

### Fase 5 - Extras y paridad completa
- [ ] Render History As GIF (crate `gif`).
- [ ] Tutoriales/Help Topics, Project News, About Paint.
- [ ] Multi-user (`sessions.js`): rediseñar con un backend open source (CRDT).
- [ ] Speech recognition y Eye-Gaze/Dwell Clicker (opcionales, probablemente externos).
- [ ] "Load From URL", "Upload to Imgur" (requieren red; opcionales).

---

## 8. Verificación de paridad (crítico)

El mayor riesgo no es "hacer un editor", es **igualar el comportamiento de jspaint**
(antialiasing, reglas de flood fill, bordes de elipse, orden de blending, etc.).

**Parity harness:**
1. Definir casos de entrada reproducibles como secuencias de gestos
   (`pointer_down/move/up` + modificadores + estado inicial) en un formato JSON.
2. El núcleo Rust los ejecuta headless y produce un PNG.
3. Un "golden runner" ejecuta lo mismo en jspaint (vía `cypress/` o un script Node
   que use el código real) y guarda el PNG esperado.
4. Comparar con métrica de píxeles (tolerancia configurable para AA) y generar diff.

Casos: cada herramienta, cada transformación, undo/redo, transparencia, patterns,
selección y texto.

Documentar explícitamente las divergencias aceptadas (p. ej. si `tiny-skia` suaviza
bordes distinto que Canvas 2D) en `plans/deviations.md`.

---

## 9. Riesgos y mitigaciones

| Riesgo | Impacto | Mitigación |
|--------|---------|-----------|
| Antialiasing/AA distinto a Canvas 2D | Paridad visual imperfecta | Paridad con tolerancia; permitir raster manual para píxel-exacto |
| Canvas2D facilita patrones/clip/paths; Rust es más manual | Más tiempo de desarrollo | Núcleo con `tiny-skia` + helpers propios; encapsular |
| Rendimiento con buffers RGBA grandes + undo por snapshot | Consumo de memoria/lag | Deltas por región, `rayon`, snapshot comprimido |
| `sessions.js` (multiplayer) depende de Firebase | Rediseño grande | Fase tardía, reemplazar por CRDT (p. ej. `automerge`/`yrs`) |
| Tool Text con métricas de fuente exactas | Diferencias de layout | Aceptar divergencia menor; usar las mismas familias de fuente |
| Alcance enorme (45 módulos) | Nunca "terminar" | MVP estricto primero; fases con criterios de salida |
| Port "lineal" del estado global JS | Código espagueti en Rust | Forzar núcleo puro + ownership; revisión con clippy |

---

## 10. Convenciones y calidad

- `cargo fmt` y `cargo clippy -- -D warnings` obligatorios.
- Errores con `Result`; nada de `unwrap()` en rutas de usuario (solo en tests).
- Cada herramienta/transformación con tests unitarios y, si aplica, snapshot.
- Benchmarks (`criterion`) para flood fill, transforms y composición.
- Commits atómicos por tarea de las checklists anteriores.

---

## 11. Hitos

| Hito | Contenido | Fase |
|------|-----------|------|
| M0 | Workspace + ventana + CI verde | 0 |
| M1 | Dibujar con lápiz/pincel y hacer undo/redo | 1 |
| M2 | Guardar y abrir PNG/BMP | 1 |
| M3 | MVP completo (criterio de salida Fase 1) | 1 |
| M4 | Transformaciones de imagen + Curve/Polygon | 2 |
| M5 | Tool Text + portapapeles | 3 |
| M6 | Paletas + temas + i18n | 4 |
| M7 | Extras y paridad completa | 5 |

---

## 12. Primeros pasos concretos (siguiente sesión)

1. `cargo new --lib crates/rspaint-core` y `cargo new crates/rspaint-app` dentro del
   workspace; configurar `[workspace]` en `rspaint/Cargo.toml`.
2. Añadir `eframe`/`egui` y dejar una ventana con un lienzo blanco de 683x384
   (los defaults de jspaint) renderizado como textura.
3. Implementar `PixelBuffer` y la herramienta Pencil junto con el historial básico
   (lineal primero; grafo en Fase 1.5).
4. Escribir el primer caso del parity harness con Pencil.
