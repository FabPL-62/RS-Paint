# RS Paint

Un port nativo a **Rust** de [JS Paint](https://github.com/1j01/jspaint), la
recreación de **MS Paint** hecha en JavaScript. RS Paint es una aplicación de
escritorio (no web) construida con `egui`/`eframe` sobre `wgpu`.

El objetivo es reproducir el comportamiento de MS Paint/JS Paint (herramientas,
selecciones, menús, historial, formatos de archivo) con lógica de imagen propia,
testeable y sin depender del navegador.

## Estado

- **Fase 0** (andamiaje): completa.
- **Fase 1** (MVP): completa — dibujo, selecciones, undo/redo, abrir/guardar.
- **Fase 2** (paridad de edición): en curso.

El plan de desarrollo por fases está en [`plans/main.md`](plans/main.md).

## Requisitos

- **Rust estable** (probado con Rust 1.90). Instálalo desde
  <https://rustup.rs/>.
- Una **GPU** con backend soportado por `wgpu` (DirectX 12 en Windows, Vulkan en
  Linux, Metal en macOS). No se necesita instalar nada extra aparte de los
  drivers.

## Compilar y ejecutar

Desde la carpeta `rspaint/`:

```sh
# Ejecutar la app (compila si hace falta)
cargo run -p rspaint-app

# Ejecutar en modo release (más rápido)
cargo run --release -p rspaint-app

# Solo compilar, sin ejecutar
cargo build -p rspaint-app
cargo build --release -p rspaint-app
```

## Tests y calidad

```sh
# Tests de todo el workspace
cargo test --workspace

# Formato
cargo fmt --all

# Lints (falla con cualquier warning)
cargo clippy --workspace --all-targets -- -D warnings
```

## Estructura del proyecto

```
rspaint/
  Cargo.toml              # workspace
  plans/main.md           # plan de desarrollo
  assets/icons/           # iconos (MIT) reutilizados de JS Paint
  crates/
    rspaint-core/         # lógica pura, sin GUI (testeable headless)
      src/
        buffer.rs         # PixelBuffer (RGBA8)
        color.rs          # Color + composición
        brush.rs          # brochas (círculo, cuadrado, diagonales)
        draw.rs           # primitivas (línea, formas, polígono)
        fill.rs           # flood fill
        selection.rs      # varita, lazo, contornos, union/resta
        transform.rs      # flip, rotate, stretch/skew, invert, crop
        history.rs        # historial no lineal (grafo)
        palette.rs        # paletas por defecto/básicas
        io.rs             # leer/escribir imágenes (PNG, BMP, JPEG, GIF, TIFF)
    rspaint-app/          # aplicación de escritorio (egui + wgpu)
      src/
        main.rs           # arranque de la ventana
        app.rs            # estado y lógica de la app
        tools.rs          # definición de herramientas
        canvas_renderer.rs# render del canvas con un pase wgpu propio
        theme.rs          # tema visual
        ui/               # menús, toolbox, color box, canvas, status bar
```

El núcleo (`rspaint-core`) **no depende de la GUI**: la app depende del núcleo y
nunca al revés. Esto permite testear toda la lógica de imagen sin abrir ventanas.

## Funcionalidades

- **Lienzo**: zoom/pan (rueda para pan, **Ctrl+rueda** para zoom anclado al
  cursor, **botón central** para mover), cuadrícula de píxeles, cursor con el
  footprint real de la herramienta.
- **Herramientas de dibujo**: Pencil, Brush (formas círculo, cuadrado y
  diagonales), Eraser / Color Eraser, Airbrush, Fill (contiguo y no contiguo),
  Pick Color, Line, Rectangle, Ellipse, Rounded Rectangle, Polygon, Magnifier.
- **Selección**: Select (rect), Free-Form Select (lazo), **Magic Wand** (con
  tolerancia), **Selection Brush**. Se pueden **sumar** (Shift) o **restar**
  (Alt / clic derecho) selecciones, mover el contenido, y tienen su propio
  undo.
- **Imagen**: Attributes (tamaño y transparencia), Clear Image, Flip/Rotate
  (incl. ángulo arbitrario), Stretch/Skew, Invert Colors, Crop to Selection.
- **Archivo**: New, Open, Save, Save As (PNG y BMP para guardar; PNG, BMP,
  JPEG, GIF y TIFF para abrir).
- **Historial**: undo/redo no lineal con ventana **History**.

### Atajos principales

- `Ctrl+Z` / `Ctrl+Y` (o `Ctrl+Shift+Z`): deshacer / rehacer.
- `Ctrl+O` / `Ctrl+S` / `Ctrl+Shift+S`: abrir / guardar / guardar como.
- `Ctrl+R` / `Ctrl+W` / `Ctrl+I`: Flip/Rotate / Stretch/Skew / Invert.
- `+` / `-` (incluye numpad): aumentar / disminuir el tamaño de la herramienta.
- Letras de herramienta: `P` pencil, `B` brush, `E` eraser, `F` fill, `L` line,
  `R` rectangle, `O` ellipse, `A` airbrush, `T` text, `I` pick color, `M`
  magnifier.
- `Delete` / `Backspace`: borrar la selección. `Escape`: cancelar/deseleccionar.
- **Shift**: restringe formas a cuadrado/círculo, líneas a 45°, y suma en
  selecciones. **Alt**: resta en selecciones.

## Licencia

MIT. Los iconos en `assets/icons/` provienen de JS Paint (también MIT).
