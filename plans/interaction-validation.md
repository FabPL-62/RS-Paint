# Validación de sesiones del editor

Fecha: 30 de octubre de 2026. Compilación `release`, sin cambios de los
algoritmos de producción durante esta validación.

Este informe conserva la línea base. La doble composición de historial se
corrigió posteriormente: las comparaciones y pruebas están en la decimocuarta
etapa de [performance.md](performance.md), con los datos nuevos separados en
[interaction-history-results.txt](interaction-history-results.txt).

## Alcance y reproducción

```powershell
cargo run --release -p rspaint-editor --example interaction_session_bench --offline
```

La sesión llama las mismas operaciones públicas del editor que utiliza la
interfaz y consume `take_pending_upload` después de cada evento. Separa el
tiempo de la acción del tiempo de composición y preparación de los bytes para
la subida. No mide eventos de Windows, GPU, vsync ni latencia hasta la pantalla.
Los escenarios son sintéticos y reproducibles, no documentos aportados por
el usuario. No representan todos los proyectos ni los modos de mezcla.

Se ejecutan cuatro escenas a 1920 × 1080 y 6000 × 4000:

- `single`: una capa blanca.
- `top`: ocho capas, activa arriba de seis capas detalladas y un fondo blanco.
- `below`: ocho capas, activa debajo de seis capas detalladas y sobre el fondo.
- `nested`: escena `top` dentro de dos grupos con opacidades 73 % y 81 %.

Las texturas usan ruido determinista RGB con alfa 140. Solo las capas estáticas
comparten almacenamiento para limitar la RAM del ensayo. La activa se separa
antes de empezar, evitando atribuir al primer trazo una copia completa causada
por compartir artificialmente su almacenamiento con las otras capas. La RAM
total de ocho imágenes independientes no queda representada por esta fixture.

Cada escena comienza después de consumir la subida inicial. Se mide un trazo
de lápiz de un píxel con 32 movimientos; un pincel circular de 256 píxeles con
suavizado 50 y 32 movimientos; 24 movimientos de control de una curva de unos
600 × 400 píxeles; cancelación de curva; seis selecciones alternadas de capa;
siete cambios de opacidad; seis cambios de visibilidad; y tres pares de
deshacer/rehacer. Inicio y fin de trazo se registran por separado del movimiento.
El fin incluye el drenaje del suavizado y la confirmación del historial.

Las muestras se ejecutan consecutivamente sin simular pausas del usuario. El
consumo de la subida termina antes del siguiente evento: no se simulan colas
de entradas ni retención de varias subidas por el renderizador. El percentil
95 usa rango más cercano; con siete o tres muestras equivale al máximo y no
debe interpretarse como una estimación estadística robusta. Las columnas de
acción y composición tienen percentiles independientes, que no deben sumarse.

Los contadores de más de 16,7 y 50 ms sirven como referencias de presupuesto
de CPU, no como mediciones de FPS o de fotogramas perdidos. Para cada sesión
se verifica la restauración exacta de la curva cancelada y de los píxeles y
el compuesto tras cada par de deshacer/rehacer, además de comparar la imagen
final completa con el compositor de referencia.

## Hallazgos para priorizar

Percentil 95 del tiempo de acción más composición/preparación, en milisegundos:

| Documento y escena | Pincel 256, movimiento | Curva, movimiento | Opacidad | Deshacer | Rehacer |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1920 × 1080, una capa | 0,410 | 1,717 | 10,519 | 2,688 | 20,130 |
| 1920 × 1080, activa arriba | 0,763 | 5,559 | 59,803 | 4,910 | 117,342 |
| 1920 × 1080, activa debajo | 12,430 | 44,567 | 391,506 | 586,196 | 611,968 |
| 1920 × 1080, grupos anidados | 2,029 | 10,233 | 85,374 | 126,108 | 269,783 |
| 6000 × 4000, una capa | 0,306 | 0,945 | 46,519 | 25,275 | 91,980 |
| 6000 × 4000, activa arriba | 2,024 | 4,056 | 656,468 | 1215,761 | 1437,293 |
| 6000 × 4000, activa debajo | 12,157 | 34,652 | 997,694 | 1707,044 | 1840,797 |
| 6000 × 4000, grupos anidados | 4,966 | 12,668 | 800,386 | 1327,797 | 1453,737 |

Son una ejecución de cada sesión y tamaños de muestra pequeños, particularmente
en las acciones de capas e historial. Los datos completos, incluidos inicio,
fin, selección de capas y bytes preparados, quedan en
[interaction-session-results.txt](interaction-session-results.txt).

La selección de capas preparó cero bytes para subir en las seis escenas con
varias capas. El máximo preparado por un movimiento de pincel fue 285 156 bytes;
las curvas prepararon hasta 1 006 068 bytes. Los cambios completos del documento
grande preparan 96 000 000 bytes por evento. Aquí no se mide su transferencia GPU.

1. **Doble composición al restaurar historial.** `Document::restore_history`
   compone inmediatamente; `Editor::undo` y `redo` luego marcan toda la imagen
   pendiente, y `take_pending_upload` vuelve a componer. El tiempo aparece
   repartido entre la acción y la preparación de subida. Es el primer candidato
   a corregir, preservando los contratos de la API y la validez del compuesto.
2. **Trabajo mediano bajo capas detalladas.** Una curva de unos 250 mil píxeles
   y los cambios completos de un documento de 1920 × 1080 quedan fuera del
   umbral paralelo de 32 MiB. La mezcla de las capas superiores domina el
   tiempo, incluso cuando el documento es menor que el caso de 6000 × 4000.
3. **Cambios completos de capas grandes.** Opacidad y visibilidad siguen
   requiriendo centenares de milisegundos con detalle. La ruta paralela mejora
   el recorrido, pero todavía no ofrece interacción fluida para ese caso.

El pico de primer trazo observado en un ensayo exploratorio desapareció al
independizar el almacenamiento activo de las capas estáticas. Ese ensayo
también tenía un parámetro de tamaño de pincel mal seleccionado; sus tiempos
se descartaron. Las cifras finales usan `brush_size = 256` y registran las
regiones transferidas para hacer visible una configuración incorrecta.

## Ventana nativa

Se compiló y abrió la aplicación real en `release`. La ventana muestra el
lienzo, panel de capas y textos corregidos. La captura tiene 1920 × 1032
píxeles, pero dos intentos de inyectar un trazo con el controlador de Windows
fallaron con `outside window bounds ... width: 16, height: 16`, incluso después
de volver a seleccionar y activar la ventana. No se completó la comprobación
de dibujo mediante ratón. Por ello esta validación no certifica la interacción
visual de extremo a extremo ni asigna tiempos a los movimientos en Windows.

No se modificaron documentos del usuario, no se guardaron dibujos ni se
cambiaron preferencias. La ventana de prueba quedó abierta con un lienzo nuevo.

## Verificación

Las ocho sesiones terminaron correctamente y pasaron sus comprobaciones de
cancelación, deshacer/rehacer e imagen final. Se compiló también la aplicación
de producción en `release`. Pasan `cargo fmt --all -- --check` y
`git diff --check`. No se repitió la batería completa del workspace porque
solo se añadieron el benchmark y los informes, sin cambios en producción.
