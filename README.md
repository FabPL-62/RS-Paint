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
- **Fase 2** (paridad de edición): prácticamente completa — transformaciones de
  imagen y de selección, Text, Curve, Polygon, redimensionado de la hoja, etc.
- **Fase 3** (texto y portapapeles): Text tool y copy/paste implementados.
- Pendiente (fases 4–5): formatos de paleta, más formatos de guardado, temas,
  persistencia y extras (GIF del historial, ayuda, multiusuario).
- **Interfaz**: tema oscuro de editor, pestañas con indicador activo, herramientas
  con iconos en dos columnas y tooltips descriptivos. Español e inglés disponibles
  desde el menú **Idioma / Language**.

El plan de desarrollo por fases está en [`plans/main.md`](plans/main.md).

## Requisitos

- **Rust estable** (probado con Rust 1.90). Instálalo desde
  <https://rustup.rs/>.
- Una **GPU** con backend soportado por `wgpu` (DirectX 12 en Windows, Vulkan en
  Linux, Metal en macOS). En Windows/macOS normalmente bastan los drivers; en
  Linux revisa el punto siguiente.
- **Linux**: además del compilador de Rust hace falta un enlazador de C
  (`build-essential` en Debian/Ubuntu, `base-devel` en Arch, `gcc` en Fedora),
  un driver de Vulkan (`mesa-vulkan-drivers` o el propietario de NVIDIA) y las
  librerías habituales del escritorio (Wayland o X11). `fontconfig` es opcional:
  si está, la herramienta Text usa las fuentes del sistema; si no, usa las
  fuentes incluidas. Los diálogos de archivo usan `xdg-desktop-portal` (KDE,
  GNOME, etc.) y, como respaldo, `zenity`.

Ejemplo en Debian/Ubuntu:

```sh
sudo apt install build-essential mesa-vulkan-drivers libvulkan1 fontconfig zenity
```

En Fedora:

```sh
sudo dnf install gcc mesa-vulkan-drivers vulkan-loader fontconfig zenity
```

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
        buffer.rs         # PixelBuffer (RGBA8), resize/escala
        color.rs          # Color + composición + HSV
        brush.rs          # brochas (círculo, cuadrado, diagonales)
        draw.rs           # primitivas (línea, formas, polígono, bézier)
        fill.rs           # flood fill
        selection.rs      # varita, lazo, contornos, union/resta
        transform.rs      # flip, rotate, stretch/skew, scale, invert, crop
        history.rs        # historial no lineal (grafo)
        palette.rs        # paletas por defecto/básicas
        io.rs             # leer/escribir imágenes (PNG, BMP, JPEG, GIF, TIFF)
    rspaint-editor/       # componente de edición sin GUI ni GPU
      src/
        lib.rs            # Editor, ToolSettings y estado privado de operaciones
        drawing.rs        # trazos, formas, curva y polígono
        text_edit.rs      # cajas de texto y preview rasterizado
        selection*.rs     # selecciones y sus transformaciones
        transforms.rs     # edición de imagen, historial y regiones modificadas
        viewport.rs       # zoom y redimensionado del lienzo
        tools.rs          # herramientas y categorías
        text.rs           # rasterizado de texto (ab_glyph)
    rspaint-app/          # aplicación de escritorio (egui + wgpu)
      assets/cursors/     # cursores por herramienta (MIT) de JS Paint
      assets/tools/       # iconos de herramientas
      locales/            # catálogos es.json y en.json
      src/
        main.rs           # arranque de la ventana
        app.rs            # composición de Editor y estado de presentación
        app/              # diálogos, entrada, archivos y presentación
        i18n.rs           # selección de idioma y resolución de mensajes
        cursors.rs        # cursores por herramienta (imágenes)
        clipboard.rs      # portapapeles del sistema (imágenes)
        canvas_renderer.rs# render del canvas con un pase wgpu propio
        theme.rs          # tema visual
        ui/               # menús, toolbox, color box, selector de color,
                          # canvas, status bar
```

El núcleo (`rspaint-core`) y el componente de edición (`rspaint-editor`)
**no dependen de la GUI**. La UI configura opciones y envía operaciones al editor.
La separación, las reglas para extenderla y cómo añadir idiomas se documentan
en [`docs/architecture.md`](docs/architecture.md).

## Funcionalidades

- **Lienzo**: zoom/pan (rueda para pan, **Ctrl+rueda** para zoom anclado al
  cursor, **botón central** para mover), cuadrícula de píxeles con opacidad,
  cursor propio por herramienta (importado de JS Paint) y **redimensionado de la
  hoja** con tiradores (borde azul + tiradores; el contenido se ancla a la arista
  opuesta y la porción nueva se rellena con el color secundario; undo/redo).
- **Zoom**: el zoom inicial ajusta la imagen a la altura del área (centrada si
  entra el ancho, o arriba-izquierda si no); **View ▸ Adjust Zoom**. El
  **Magnifier** permite arrastrar un rectángulo para hacer zoom a esa zona
  (limitado al máximo), además del clic para acercar/alejar.
- **Herramientas de dibujo**: Pencil, Brush (círculo, cuadrado y diagonales),
  Eraser (círculo/cuadrado) / Color Eraser, Airbrush, Fill (contiguo y no
  contiguo), Pick Color, Line, **Curve** (2 puntos de control), Rectangle,
  Ellipse, Rounded Rectangle, **Polygon** (con Width y estilo), **Text**
  (familias Sans/Serif/Mono, tamaño, negrita/cursiva, vertical, transparente;
  el preview se rasteriza en vivo y coincide con lo confirmado).
- **Selección**: Select (rect), Free-Form Select (lazo), **Magic Wand** (con
  tolerancia), **Selection Brush**, **Contour Select** (rectángulo quitando los
  píxeles del color secundario) y **Outliner**. Se pueden **sumar** (Shift) o
  **restar** (Alt / **clic derecho**) selecciones, mover el contenido, y tienen
  su propio undo. Cada selección muestra un **recuadro de transformación** con
  tiradores (redimensiona el contenido) y un **gizmo de rotación** (cruz de
  pivote + línea y círculo; arrastrar el círculo gira el contenido, con snap a
  45° con **Shift**).
- **Transformaciones**: Flip/Rotate (incl. ángulo arbitrario) y Stretch/Skew se
  aplican **a la selección** si hay una (si no, a toda la imagen); desde las
  opciones de selección hay botones directos (Flip H/V, 90°/180°/270°).
- **Portapapeles**: **Ctrl+C** copia la selección (al portapapeles interno y al
  del sistema); **Ctrl+V** pega (acepta imágenes del portapapeles del sistema),
  centrado en el cursor y como selección flotante que se confirma al hacer clic
  fuera.
- **Paleta**: selector avanzado **HSV** (cuadrado S/V, tira de hue, barra de
  valor) en un popup junto al color; la paleta es editable (long-press edita un
  cuadro). Cada imagen conserva sus colores primario/secundario y sus paletas.
  La paleta tradicional tiene 2 filas de 14 colores y **Reset** debajo.
  A su derecha, **Capture** recoge los colores distintos de la imagen y los
  ordena por tono; muestra 2 filas de 14 colores, con flechas para recorrerlas.
  Al abrir una imagen, el color secundario toma el píxel superior izquierdo.
  La opacidad se ajusta desde el selector de color. Clic derecho en el
  indicador intercambia primario/secundario.
- **Imagen**: Attributes (tamaño y transparencia), Clear Image, Invert Colors,
  Crop to Selection.
- **Archivo**: New, Open, Save, Save As (PNG y BMP para guardar; PNG, BMP,
  JPEG, GIF y TIFF para abrir). Al cerrar con cambios sin guardar se pide
  confirmación (Cancelar / Guardar y salir / Salir).
  Abrir una imagen crea y activa una pestaña nueva, conservando el documento
  anterior y sus cambios sin guardar. Si la pestaña temporal inicial sigue
  intacta y en blanco, se reutiliza para la primera imagen abierta.
  También se pueden arrastrar y soltar imágenes sobre la ventana: la primera
  reutiliza una hoja base intacta y las demás se abren en pestañas nuevas.
- **Historial**: undo/redo no lineal con ventana **History**.

### Atajos principales

- `Ctrl+Z` / `Ctrl+Y` (o `Ctrl+Shift+Z`): deshacer / rehacer.
- `Ctrl+C` / `Ctrl+V`: copiar / pegar la selección.
- `Ctrl+O` / `Ctrl+S` / `Ctrl+Shift+S`: abrir / guardar / guardar como.
- `Ctrl+R` / `Ctrl+W` / `Ctrl+I`: Flip/Rotate / Stretch/Skew / Invert.
- `Ctrl+E`: seleccionar todo (con el Outliner, aplicar a toda la imagen).
- `+` / `-` (incluye numpad): aumentar / disminuir el tamaño de la herramienta.
- Letras de herramienta: `P` pencil, `B` brush, `E` eraser, `F` fill, `L` line,
  `C` curve, `R` rectangle, `O` ellipse, `A` airbrush, `T` text, `I` pick
  color, `M` magnifier.
- **Ctrl** (con herramienta de dibujo): pick temporal del color primario;
  **Alt**: del secundario.
- `Delete` / `Backspace`: borrar la selección. `Escape`: cancelar/deseleccionar.
- **Shift**: restringe formas a cuadrado/círculo, líneas a 45°, ajusta el giro a
  45°, y suma en selecciones. **Alt**: resta en selecciones.

## Licencia

MIT. Los iconos en `assets/icons/` provienen de JS Paint (también MIT).
