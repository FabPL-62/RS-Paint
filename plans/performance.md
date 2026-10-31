# Rendimiento

## Vigésima primera etapa: regiones separadas y subidas por lotes

Los cambios de opacidad, visibilidad y modo de fusión pueden conservar hasta
32 regiones separadas de contenido. Se usan los límites por bloque ya conocidos
y se unen los rectángulos que se tocan. Solo se elige el lote si su área total
es menor que un cuarto de la caja global. Con más fragmentación o cobertura
densa se vuelve a la caja única, manteniendo un número acotado de operaciones.

No se fuerza un escaneo espacial de imágenes densas solo para intentar dividir
su caja: se aprovechan metadatos por bloque existentes o una mayoría conocida
de bloques inicialmente transparentes. Las capas importadas sin esa información
y con contenido en los cuatro bordes pueden conservar la ruta de caja única.
Los grupos respetan las mismas reglas de visibilidad y alfa de las etapas previas.

El editor incorpora `take_pending_uploads`, que compone y prepara cada región
por separado. Los trazos pendientes se unen con esas regiones; invalidaciones
completas e historial siguen teniendo prioridad. Los cambios repetidos del
mismo deslizador se coalescen, y el total pendiente también se limita a 32.
Las capas que ocupan todo el lienzo conservan una subida completa compartida.

`take_pending_upload` permanece compatible: consume el lote y entrega una sola
imagen que engloba todas sus partes. Ese cliente puede seguir preparando una
caja grande, aunque la composición ya se haya hecho por zonas. El renderizador
de producción usa la API de lotes y ejecuta `queue.write_texture` para cada
región. La cola pendiente todavía recurre a una instantánea completa si recibe
otra actualización antes de que el callback consuma la anterior: conserva la
política segura previa frente a pases descartados y cambios de documento/tamaño.

### Dos parches alejados

```powershell
cargo run --release -p rspaint-editor --example sparse_layer_changes_bench --offline -- --separated
```

La fixture añade un segundo parche de 120 × 80 cerca de la esquina opuesta,
manteniendo ocho capas y los fondos detallados compartidos del ensayo anterior.
La medición consume la API de lotes tanto antes (entonces una sola región)
como después del cambio. Medianas CPU de siete ajustes de opacidad y seis
cambios de visibilidad, incluyendo composición y preparación de bytes:

| Escena | Opacidad anterior → actual | Visibilidad anterior → actual |
| --- | ---: | ---: |
| 1920 × 1080, arriba | 2,592 → 0,185 ms | 2,099 → 0,186 ms |
| 1920 × 1080, debajo | 14,647 → 1,026 ms | 12,938 → 1,014 ms |
| 1920 × 1080, grupo | 3,288 → 0,274 ms | 3,359 → 0,275 ms |
| 6000 × 4000, arriba | 214,104 → 0,249 ms | 206,223 → 0,248 ms |
| 6000 × 4000, debajo | 274,459 → 1,337 ms | 256,647 → 1,341 ms |
| 6000 × 4000, grupo | 200,338 → 0,206 ms | 199,447 → 0,207 ms |

La caja grande de 6000 × 4000 preparaba 73 440 000 bytes; los dos rectángulos
preparan 76 800 bytes. En 1920 × 1080 pasa de 2 534 400 a 76 800 bytes.
Son resultados de esta distribución espacial, no una mejora universal de capas
dispersas. Las ocho escenas comparan la imagen final completa con la referencia.
Se repiten también las ocho escenas de un parche/vacías como control.

Datos: [coverage-regions-before-results.txt](coverage-regions-before-results.txt),
[coverage-regions-results.txt](coverage-regions-results.txt) y
[coverage-regions-control-results.txt](coverage-regions-control-results.txt).

### Callback GPU real, fuera de pantalla

```powershell
cargo run --release -p rspaint-app --example gpu_pipeline_bench --offline -- --case=separated-opacity-8
```

RTX 3060 Ti, Vulkan, controlador 621.01; dos muestras de calentamiento y siete
medidas. Los parches cambian de opacidad, sin añadir el trazo que utilizan otros
escenarios del benchmark. Son medianas independientes, no columnas sumables:

| Documento | CPU composición/preparación | Encolado de subida | Envío hasta finalización | Dibujo GPU | Pipeline completo |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1920 × 1080 | 0,250 ms | 0,030 ms | 0,180 ms | 0,031 ms | 0,511 ms |
| 6000 × 4000 | 0,191 ms | 0,016 ms | 0,181 ms | 0,070 ms | 0,416 ms |

El percentil 95 del pipeline grande fue 0,464 ms. No hay una medición anterior
de este mismo escenario GPU para atribuir una mejora del pipeline; la tabla
anterior de CPU es una comparación distinta. Se excluyen eventos de ventana,
vsync y presentación. No se hace lectura de píxeles GPU en este ensayo; la
exactitud de los lotes se comprueba en CPU al aplicarlos sobre una textura
simulada y mediante referencias completas del editor.
Datos: [coverage-regions-gpu-results.txt](coverage-regions-gpu-results.txt).

### Verificación y siguiente caso

Las pruebas cubren los trece modos y grupos con puntos en extremos opuestos,
limpieza al ocultar, conservación de huecos, fusiones repetidas de pendientes,
trazos intermedios, restauración de historial y compatibilidad de la API de una
imagen. Se comprueba el límite de fragmentación y que lotes sin renderizar
seguidos de otro cambio no pierdan píxeles. No se cambian los presupuestos de
caché ni el número máximo de hilos.

Siguiente caso: acumular actualizaciones anteriores al render sin tener que
volver a una imagen completa para conservar todos los cambios pendientes.

Verificación: `cargo test --workspace --offline` pasa 221 pruebas y deja una
de portapapeles ignorada. El ejemplo GPU pasa cuatro pruebas adicionales.
Pasan `cargo fmt --all -- --check` y `git diff --check`.

## Vigésima etapa: primer cálculo con el relleno inicial conocido

Cada `PixelBuffer` conserva la generación y el alfa de su última creación o
relleno uniforme. Un bloque que aún tiene esa generación conserva ese alfa:
no hay que leer sus píxeles para conocer sus límites. Una capa vacía sin
modificaciones se reconoce mirando solo la tabla pequeña de versiones.

La generación reservada para la siguiente escritura es distinta de la del
relleno. Así, incluso la primera escritura sin historial ni caché invalida
la prueba del bloque tocado. `fill` y `clear` vuelven a establecer una prueba
uniforme y reservan otra generación para futuras modificaciones. Se añaden
solo una generación y un alfa por buffer, sin nuevos mapas de píxeles.

Se revisaron todos los accesos internos directos al almacenamiento. Recorte
y redimensionado marcan como modificados los bloques copiados; el área nueva
que conserva el relleno sigue siendo conocida. El acceso crudo mediante
`data_mut` invalida todos los bloques. Las copias COW preservan su prueba inicial
pero sus escrituras divergen con las generaciones existentes.

La caché de cobertura combina ahora tres fuentes exactas: alfa conocido del
relleno para bloques intactos, límites previamente calculados con la misma
versión y lectura de los bloques restantes. Esto permite resolver la primera
consulta de una capa recién dibujada sin necesitar una entrada previa en caché.
Se conservan los límites de 64 entradas y 4 MiB de vectores de metadatos.

### Primer cambio de opacidad

Se añade una escena de capa vacía al benchmark. Se midieron las mismas ocho
escenas antes y después con:

```powershell
cargo run --release -p rspaint-editor --example sparse_layer_changes_bench --offline
```

Tiempo CPU del primer ajuste: acción, cálculo de límites, composición y
preparación de bytes. El lienzo ya había sido compuesto inicialmente, pero
la cobertura de la capa aún no había sido consultada. El parche se crea con
escrituras por fila sobre una capa inicialmente transparente.

| Escena, ocho capas | Primer ajuste anterior → actual | Bytes preparados actuales |
| --- | ---: | ---: |
| 1920 × 1080, parche arriba | 2,213 → 0,216 ms | 38 400 |
| 1920 × 1080, parche debajo | 2,700 → 0,629 ms | 38 400 |
| 1920 × 1080, parche en grupo | 2,103 → 0,158 ms | 38 400 |
| 1920 × 1080, capa vacía | 2,097 → 0,003 ms | 0 |
| 6000 × 4000, parche arriba | 26,028 → 2,022 ms | 38 400 |
| 6000 × 4000, parche debajo | 23,641 → 0,849 ms | 38 400 |
| 6000 × 4000, parche en grupo | 24,573 → 1,983 ms | 38 400 |
| 6000 × 4000, capa vacía | 17,788 → 0,004 ms | 0 |

Son primeras consultas individuales, no percentiles ni garantías. Las zonas
ya analizadas conservan sus tiempos posteriores y la subida regional. No se
incluyen GPU, eventos nativos ni presentación. Se repiten también las ocho
escenas con `--after-paint` como control; la escena inicialmente vacía recibe
un punto nuevo en cada muestra y termina preparando como máximo 28 bytes.
Las dieciséis escenas posteriores y las ocho anteriores comparan el compuesto
completo con la referencia.

Datos: [coverage-seed-before-results.txt](coverage-seed-before-results.txt),
[coverage-seed-results.txt](coverage-seed-results.txt) y
[coverage-seed-paint-control-results.txt](coverage-seed-paint-control-results.txt).

### Verificación y límites

Las pruebas verifican que no se lean píxeles para la primera consulta vacía
y que un punto previo a la primera consulta obligue a leer solo su bloque.
Se comprueba la prueba de alfa contra los píxeles de todos los bloques que
se declaran conocidos después de escribir, rellenar, clonar, recortar y
redimensionar con y sin desplazamiento. El historial, escritura cruda y
restauraciones mantienen las pruebas existentes de igualdad.

La mejora depende de conservar la prueba del relleno. Los datos importados
por acceso crudo, algunas reconstrucciones y los bloques realmente escritos
siguen pudiendo requerir lectura. El presupuesto de metadatos y el rectángulo
global de la capa siguen limitando los casos optimizables.

Siguiente caso: capas con contenido en zonas muy separadas, que todavía se
engloban en un rectángulo grande aunque la mayor parte esté vacía.

Verificación: `cargo test --workspace --offline` pasa 217 pruebas y deja una
de portapapeles ignorada. Pasan `cargo fmt --all -- --check` y `git diff --check`.

## Decimonovena etapa: límites incrementales tras pintar

La caché de cobertura conserva ahora límites por bloque de 128 × 128 y las
versiones inmutables que ya usa el historial. Tras una edición, busca una
entrada de las mismas dimensiones con versiones coincidentes y reutiliza los
límites de esos bloques, incluidos los vacíos. Solo lee los píxeles de bloques
cuyas versiones han cambiado. Los límites globales se vuelven a unir, de modo
que también pueden reducirse al borrar contenido. No se guardan píxeles de origen.

Los aciertos de identidad débil continúan siendo inmediatos. La prueba de
bordes para imágenes densas conserva su ruta rápida. Las entradas cuya fuente
de píxeles ya murió pueden aportar versiones y límites válidos de bloques;
por eso se retienen hasta su expulsión, sin retener sus buffers RGBA.

Se conservan como máximo 64 entradas. Las capacidades de los vectores de
límites y versiones tienen además un presupuesto contabilizado de 4 MiB por
documento; las tablas compartidas se cuentan conservadoramente por entrada.
Entradas que excederían por sí solas ese presupuesto conservan solo el límite
global y recurren a una inspección completa. Esto no cambia los 32 MiB de
píxeles de la caché de composición. Objetos, referencias débiles, asignador y
temporales para construir una nueva entrada no forman parte de esos 4 MiB.

Retener una tabla de versiones fuerza su separación antes de la siguiente
escritura, usando el mecanismo COW existente. De ese modo, una misma versión
significa los mismos píxeles de bloque, incluso entre clones y restauraciones
de historial. Un acceso crudo mediante `data_mut` invalida todos los bloques:
la optimización no supone que una escritura arbitraria sea local.

### Medición del flujo con pintura

```powershell
cargo run --release -p rspaint-editor --example sparse_layer_changes_bench --offline -- --after-paint
```

Se conservan las seis escenas de la etapa 18. Después del primer cambio de
opacidad, cada muestra incluye pintar un punto nuevo dentro del parche,
confirmar el trazo en el historial, cambiar opacidad y preparar la subida.
Se usan las APIs de trazo del editor, que conservan pendiente la zona pintada
para unirla con la invalidación de metadatos. La línea base usa la caché global
de la etapa 18 con exactamente la misma opción de benchmark.

| Escena, parche 120 × 80 | Mediana anterior → actual | Máximo anterior → actual |
| --- | ---: | ---: |
| 1920 × 1080, arriba | 1,515 → 0,160 ms | 1,709 → 0,206 ms |
| 1920 × 1080, debajo | 2,157 → 0,630 ms | 2,537 → 0,672 ms |
| 1920 × 1080, grupo | 1,692 → 0,241 ms | 2,130 → 0,357 ms |
| 6000 × 4000, arriba | 15,332 → 0,281 ms | 15,562 → 0,326 ms |
| 6000 × 4000, debajo | 15,774 → 0,899 ms | 16,730 → 0,951 ms |
| 6000 × 4000, grupo | 17,114 → 0,375 ms | 19,869 → 0,438 ms |

Son medianas de siete eventos consecutivos de pintura más opacidad y
composición/preparación en CPU, sin GPU, eventos de ventana ni presentación.
Los cambios siguen preparando 38 400 bytes. Cada escena verifica el compuesto
final completo contra la referencia. Se mantiene también el control de seis
escenas sin pintura entre ajustes, con sus comparaciones exactas.

Datos: [coverage-incremental-before-results.txt](coverage-incremental-before-results.txt),
[coverage-incremental-results.txt](coverage-incremental-results.txt) y
[coverage-incremental-control-results.txt](coverage-incremental-control-results.txt).

### Límites y siguiente caso

La primera consulta sin metadatos previos todavía inspecciona toda una capa
dispersa. En el ensayo con pintura, el primer cambio grande pasó de 16–18 ms
a 18–25 ms según la escena: no se mejora ese caso y construir los límites por
bloque puede añadir trabajo. La ganancia requiere una entrada compatible
anterior; tras expulsión, cambio de dimensiones, escritura cruda o modificación
de todos los bloques puede hacer falta un recorrido completo.

Las pruebas verifican contadores de bloques leídos sin depender del reloj:
un punto cambia uno de nueve bloques, una fila que cruza el borde cambia dos,
y `fill` y `data_mut` vuelven a inspeccionarlos todos. También cubren borrado,
clones divergentes, restauración del historial y expulsión por presupuesto de
metadatos, comprobando que no sobreviven propietarios fuertes de los píxeles.

Siguiente caso: reducir la primera consulta de capas nuevas, donde aún no
existen límites anteriores que se puedan reutilizar.

Verificación: `cargo test --workspace --offline` pasa 214 pruebas y deja una
de portapapeles ignorada. Pasan `cargo fmt --all -- --check` y `git diff --check`.

## Decimoctava etapa: cambios de capas con contenido localizado

Opacidad, visibilidad y modo de fusión invalidan ahora el rectángulo que engloba
los píxeles con alfa distinto de cero del nodo, recortado al lienzo. Las capas
vacías actualizan metadatos e historial sin solicitar una subida de píxeles.
Si el rectángulo abarca todo el lienzo, se conserva la ruta completa que comparte
el `Arc` del compuesto; no se copia una imagen entera mediante la ruta regional.

`Document::layer_content_bounds` obtiene los píxeles activos del buffer de
trabajo, incluso durante una vista previa con marcador de 1 × 1 en el árbol.
Para grupos une el contenido de sus descendientes visibles y con opacidad
positiva. Ignora las propiedades propias del nodo consultado: volver a mostrar
un nodo o subir su opacidad debe poder exponer todo su contenido. Los modos
actuales operan por píxel y no expanden ese rectángulo. Los RGB bajo alfa cero
no cuentan como contenido que pueda alterar el compuesto.

La caché nueva guarda como máximo 64 identidades de buffer y sus rectángulos,
incluidos resultados vacíos. Usa referencias débiles, dimensiones y antigüedad,
sin retener los bytes de origen. Cualquier mutación mediante las APIs del buffer
invalida su identidad débil; cambios de visibilidad de los descendientes se
vuelven a evaluar al unir un grupo. La caché se clona fría y se limpia al
reemplazar una imagen. No modifica el presupuesto de 32 MiB de píxeles del
compositor: añade metadatos acotados, contabilizados aparte.

Una consulta sin metadatos puede recorrer toda la capa. Cuando se prueba que
hay alfa no nulo en los cuatro bordes, el rectángulo completo se determina sin
recorrer el interior; una imagen densa suele necesitar solo cuatro píxeles.
Una modificación posterior exige aprender los límites otra vez. Los cambios
de metadatos sobre píxeles que permanecen iguales reutilizan el rectángulo.

### Medición de capas dispersas

```powershell
cargo run --release -p rspaint-editor --example sparse_layer_changes_bench --offline
```

Ocho capas, seis texturas estáticas detalladas compartidas para limitar RAM,
un fondo blanco y una capa transparente con un parche de 120 × 80 y alfa 180.
Se prueba la capa arriba, debajo de las texturas y dentro de un grupo aislado
que contiene solo esa capa. El primer cambio mide también la consulta inicial
de límites. Después se miden siete cambios de opacidad y seis de visibilidad.
Los tiempos incluyen acción, composición y bytes preparados, sin GPU ni ventana.

| Escena | Opacidad, mediana anterior → actual | Visibilidad, mediana anterior → actual | Primer cambio actual |
| --- | ---: | ---: | ---: |
| 1920 × 1080, arriba | 4,476 → 0,123 ms | 4,293 → 0,124 ms | 1,447 ms |
| 1920 × 1080, debajo | 29,255 → 0,686 ms | 29,158 → 0,677 ms | 2,293 ms |
| 1920 × 1080, grupo | 6,248 → 0,137 ms | 5,963 → 0,134 ms | 1,522 ms |
| 6000 × 4000, arriba | 256,999 → 0,122 ms | 247,668 → 0,124 ms | 17,049 ms |
| 6000 × 4000, debajo | 301,311 → 0,501 ms | 295,522 → 0,504 ms | 12,591 ms |
| 6000 × 4000, grupo | 232,790 → 0,136 ms | 229,614 → 0,139 ms | 16,910 ms |

En el documento grande, cada subida preparada pasa de 96 000 000 a 38 400 bytes,
2500 veces menos bytes RGBA. No es una medición de transferencia GPU. Los
resultados solo representan esta distribución localizada; una capa densa o
manchas separadas con un rectángulo grande no reciben el mismo ahorro. La
primera consulta sigue siendo mucho más costosa que las posteriores.
Cada escena compara el compuesto completo final con la referencia.

Datos: [sparse-layer-before-results.txt](sparse-layer-before-results.txt) y
[sparse-layer-results.txt](sparse-layer-results.txt). También se repiten las
ocho sesiones originales como control denso, con comprobaciones de píxeles:
[sparse-layer-dense-control-results.txt](sparse-layer-dense-control-results.txt).
No se atribuye una mejora a sus variaciones entre ejecuciones.

### Verificación y siguiente caso

Las pruebas cubren los trece modos sobre capas y grupos, limpiar los píxeles al
ocultar, RGB con alfa cero, escrituras que amplían o reducen los límites, clones
que divergen, acceso por fila y acceso crudo, grupos ocultos, opacidad cero,
el buffer activo durante un trazo, unión con cambios regionales pendientes y
conservación de una invalidación completa anterior. Las capas vacías mantienen
su historial y no borran otras actualizaciones pendientes. La ruta densa prueba
la identidad del `Arc` subido para conservar la subida completa compartida.

Siguiente caso: reducir el primer cálculo de límites después de pintar, que
todavía puede recorrer toda una capa grande aunque solo haya cambiado una zona.

Verificación: `cargo test --workspace --offline` pasa 211 pruebas y deja una
de portapapeles ignorada. Pasan `cargo fmt --all -- --check` y `git diff --check`.

## Decimoséptima etapa: redondeo general, transparencia y otros modos

La conversión exacta de la etapa 16 se aplica ahora también al compositor
genérico: alfa sobre un destino vacío, alfa resultante y canales RGB de los
trece modos. No cambian las funciones de fusión, su orden de cálculo ni el
muestreo espacial de Disolver. La fórmula se comparte entre dos instancias
con parámetro constante: producción usa la conversión nueva y las pruebas
mantienen una instancia con `round().clamp()` como oráculo. No hay un selector
de estrategia en tiempo de ejecución ni almacenamiento adicional.

La prueba nueva compara 159 744 combinaciones: los trece modos, los 256 alfas
de origen, ocho alfas de destino, seis opacidades y coordenadas globales que
varían con los colores. Las comparaciones por píxel existentes usan también
el oráculo con redondeo antiguo. Se conserva la prueba de más de un millón
de patrones f32 y los límites semienteros de la etapa anterior.

### Sesiones con modos combinados

Se añade `--mixed` al benchmark de sesiones. Las siete capas de textura usan
cíclicamente Normal, Multiplicar, Trama y Superponer, con el mismo ruido RGB
y alfa 140 de las escenas originales. Las escenas con una sola capa siguen
siendo controles Normal. Se midieron las ocho escenas antes y después de
modificar el compositor, sin cambiar la fixture entre ambas ejecuciones:

```powershell
cargo run --release -p rspaint-editor --example interaction_session_bench --offline -- --mixed
```

Mediana de siete cambios de opacidad, acción y composición/preparación en CPU:

| Escena con modos combinados | Anterior → actual |
| --- | ---: |
| 1920 × 1080, una capa (control) | 9,604 → 6,164 ms |
| 1920 × 1080, activa arriba | 26,474 → 24,200 ms |
| 1920 × 1080, activa debajo | 83,306 → 65,773 ms |
| 1920 × 1080, grupos anidados | 86,548 → 80,968 ms |
| 6000 × 4000, una capa (control) | 53,456 → 54,001 ms |
| 6000 × 4000, activa arriba | 751,813 → 529,155 ms |
| 6000 × 4000, activa debajo | 892,298 → 654,576 ms |
| 6000 × 4000, grupos anidados | 792,552 → 570,269 ms |

Los tres casos grandes con varias capas reducen esta mediana alrededor de
27–30 %. Son pocas muestras y una ejecución por configuración; no se atribuye
cada diferencia de los controles al cambio de redondeo. Datos completos de
las líneas base y posterior en
[interaction-mixed-before-results.txt](interaction-mixed-before-results.txt) y
[interaction-mixed-results.txt](interaction-mixed-results.txt).

### Control Normal y grupos con transparencia

Se repiten además las ocho escenas Normal, con datos en
[interaction-generic-results.txt](interaction-generic-results.txt). Frente a la
etapa 16, la mediana de opacidad de la escena grande con activa arriba pasa
de 267,172 a 262,235 ms: la ruta especializada previa se conserva. En grupos
anidados, la mediana pasa de 87,039 a 57,482 ms en 1920 × 1080 y de 397,922
a 344,569 ms en 6000 × 4000. No se promete una mejora uniforme para todos los
documentos o modos: divisiones, mezcla y composición completa siguen teniendo
coste propio.

Las dieciséis sesiones posteriores pasan las comparaciones de vista previa,
cancelación, historial e imagen final. Las ocho sesiones mixtas anteriores
también pasan sus comprobaciones. No se miden eventos de ventana, GPU ni
presentación. La caché y sus límites, el número máximo de hilos y la política
de admisión no se modifican en esta etapa.

Siguiente caso: evitar que cambiar opacidad o visibilidad de una capa con
contenido en una región pequeña obligue a recomponer y subir todo el lienzo.

Verificación: `cargo test --workspace --offline` pasa 204 pruebas y deja una
de portapapeles ignorada. Pasan `cargo fmt --all -- --check` y `git diff --check`.

## Decimosexta etapa: conversión exacta a bytes en Normal sobre fondo opaco

La ruta especializada de mezcla Normal conserva la aritmética normalizada y
su orden, pero sustituye la conversión final `round().clamp(... ) as u8` por
`(f64::from(value) + 0.5) as u8`. Solo se ensancha el valor final ya calculado;
no se cambia el cálculo de alfa o color anterior. La referencia genérica
`blend_pixel` conserva el redondeo original.

Para los valores que pueden cambiar un byte entre 0 y 255, el ensanchamiento
preserva exactamente el valor f32 y permite sumar 0,5 sin perder la distancia
a los límites de redondeo. La conversión a u8 trunca y satura. Los negativos
y NaN terminan en cero, y los valores altos e infinitos positivos en 255, igual
que la expresión de referencia. Es importante ensanchar antes de sumar:
sumar 0,5 en f32 puede llevar el predecesor de un semientero al entero siguiente
y cambiar un píxel. No se aplica una aproximación entera a la mezcla.

La prueba nueva compara todos los 256 semienteros relevantes, cuatro valores
representables a cada lado, sus negativos, ceros, infinitos, NaN y extremos,
además de 1 048 576 patrones de bits pseudoaleatorios. Se mantienen las pruebas
de la mezcla especializada contra `blend_pixel` para alfa, opacidad y canales.
No se añade almacenamiento, caché ni hilos.

### Sesiones repetidas

Se ejecuta `interaction_session_bench` con las mismas ocho escenas. Medianas
de acción y composición/preparación de subida en CPU; la línea base es la
etapa 15, guardada en `interaction-medium-results.txt`:

| Escena | Opacidad anterior → actual | Visibilidad anterior → actual |
| --- | ---: | ---: |
| 1920 × 1080, una capa | 9,945 → 6,500 ms | 9,782 → 6,454 ms |
| 1920 × 1080, activa arriba | 58,947 → 21,457 ms | 57,820 → 19,803 ms |
| 1920 × 1080, activa debajo | 75,317 → 34,663 ms | 73,841 → 33,448 ms |
| 1920 × 1080, grupos anidados | 118,069 → 87,039 ms | 84,624 → 87,068 ms |
| 6000 × 4000, una capa | 43,070 → 53,794 ms | 43,994 → 55,362 ms |
| 6000 × 4000, activa arriba | 578,140 → 267,172 ms | 587,681 → 275,807 ms |
| 6000 × 4000, activa debajo | 767,038 → 342,022 ms | 764,895 → 342,363 ms |
| 6000 × 4000, grupos anidados | 683,009 → 397,922 ms | 651,072 → 339,797 ms |

Las escenas grandes con ocho capas y fondo opaco mejoran alrededor de 2,1–2,2
veces en opacidad/visibilidad con activa arriba o debajo. Los casos de una
capa o mezclas sobre transparencia no son el objetivo de la especialización;
sus diferencias entre ejecuciones no se atribuyen a ella. Las medianas de
curvas con activa debajo pasan de 13,524 a 6,926 ms en 1920 × 1080 y de
11,789 a 5,826 ms en 6000 × 4000. Las pausas de cambios completos grandes
persisten, aunque menores.

Son siete cambios de opacidad, seis de visibilidad y 24 movimientos de curva
por escena, con los límites de muestreo descritos en el informe de validación.
No se incluyen GPU, eventos nativos ni presentación. Las ocho sesiones pasan
las comparaciones exactas de vista previa, cancelación, historial e imagen final.
Datos completos: [interaction-rounding-results.txt](interaction-rounding-results.txt).

Verificación: `cargo test --workspace --offline` pasa 203 pruebas y deja una
de portapapeles ignorada. Pasan `cargo fmt --all -- --check` y `git diff --check`.

## Decimoquinta etapa: curvas bajo capas detalladas y regiones medianas

El coste identificado en las curvas era principalmente la recomposición de
las capas superiores sobre el resultado activo. No se cambia el cálculo de
curvas, el pincel ni su suavizado. Se amplía la admisión de recorridos paralelos
a regiones de más de 131 072 píxeles que contienen capas visibles superiores
al activo cuya mezcla depende de él y no puede reutilizarse como un fondo.
Se recorren también los grupos que contienen la rama activa, ignorando las
ramas ocultas y de opacidad cero.

En esos casos, la primera fila de temporales tiene 64 píxeles de alto en vez
de 256: así no se consume la mayor parte de una vista previa antes de decidir.
Solo se distribuye el resto si hay al menos 16 384 píxeles de muestra y 65 536
pendientes, la muestra tarda al menos 2 ms, su coste normalizado supera o iguala
50 ms por 1 048 576 píxeles y el coste serial restante estimado alcanza 8 ms.
Las regiones pequeñas y los recorridos baratos siguen en serie. Para más de
8 388 608 píxeles pendientes se conserva el criterio grande de la etapa 13.

La restricción de topología es necesaria: un ensayo exploratorio sin ella
ralentizaba cambios de opacidad con la activa arriba, porque evitaba llenar
una caché que podía almacenar todo el fondo de 1920 × 1080. La versión final
no usa esta nueva prueba mediana cuando el trabajo se limita al activo y
fondos o interiores de grupos estáticos cacheables. La política de caché para
recorridos grandes sigue disponible por separado.

### Sesiones repetidas

`cargo run --release -p rspaint-editor --example interaction_session_bench --offline`
repite las ocho escenas. La siguiente tabla usa las medianas de los 24
movimientos de control de curva: tiempo CPU de acción y composición/preparación
de subida, sin GPU ni latencia de ventana. La línea base es la ejecución de
la etapa 14, conservada en `interaction-history-results.txt`.

| Curva, escena | Mediana anterior → actual | Percentil 95 anterior → actual |
| --- | ---: | ---: |
| 1920 × 1080, una capa | 1,263 → 1,493 ms | 1,373 → 1,669 ms |
| 1920 × 1080, activa arriba | 4,652 → 5,449 ms | 4,802 → 5,706 ms |
| 1920 × 1080, activa debajo | 35,190 → 13,524 ms | 36,884 → 14,054 ms |
| 1920 × 1080, grupos anidados | 9,036 → 12,272 ms | 9,468 → 12,373 ms |
| 6000 × 4000, una capa | 1,033 → 0,834 ms | 1,089 → 0,955 ms |
| 6000 × 4000, activa arriba | 5,148 → 3,795 ms | 5,476 → 4,116 ms |
| 6000 × 4000, activa debajo | 43,563 → 11,789 ms | 44,293 → 11,997 ms |
| 6000 × 4000, grupos anidados | 8,936 → 8,727 ms | 10,034 → 8,939 ms |

Los dos casos objetivo mejoran unas 2,6 y 3,7 veces. Sus 24 movimientos quedan
por debajo de 16,7 ms de CPU en esta ejecución; eso no garantiza 60 FPS en la
ventana. Los demás casos no reciben esta nueva ruta mediana y las diferencias
entre ejecuciones no se atribuyen a una optimización. Se conservan los datos
completos, incluidos los controles y la variación, en
[interaction-medium-results.txt](interaction-medium-results.txt).

El cambio también permite repartir cambios completos de una imagen de
1920 × 1080 cuando hay capas costosas por encima del activo. Las subidas
completas grandes aún pueden causar pausas: esta etapa no elimina ese coste.

### Exactitud y límites

El benchmark ahora compara además toda la imagen de la curva antes de
cancelarla, fuera del tiempo medido. Así no basta con que la cancelación
restaure correctamente una vista previa potencialmente incorrecta. Se siguen
comprobando cancelación, deshacer/rehacer e imagen final de cada sesión.

Las pruebas nuevas comprueban admisión con duraciones suministradas, exclusión
de ramas ocultas y cacheables y composición mediana con origen no alineado,
recorte de bordes, grupos y los trece modos contra la referencia. Se mantiene
el límite de ocho hilos y la misma caché acotada; no se retiene información de
tiempos entre movimientos. Los tiempos de pared y la estimación del prefijo
pueden elegir una estrategia menos rentable cuando el contenido o la carga
del sistema varían dentro del recorrido, conservando los píxeles exactos.

Verificación: `cargo test --workspace --offline` pasa 202 pruebas y deja una
de portapapeles ignorada. Pasan `cargo fmt --all -- --check` y `git diff --check`.

## Decimocuarta etapa: una sola composición al restaurar historial

`Document::restore_history` sigue dejando el compuesto completo calculado,
conservando el contrato público y la posibilidad de muestrearlo inmediatamente.
El editor distingue ahora una subida completa cuyo compuesto ya está listo de
una invalidación que todavía exige recomponer. Deshacer, rehacer y saltar a un
nodo del historial usan esta subida lista; `take_pending_upload` comparte sus
píxeles sin volver a recorrer las capas.

Una nueva invalidación completa o regional revoca la reutilización. Esto
incluye pintar, cambiar opacidad, cambiar de documento o invalidar el lienzo
antes de consumir la subida del historial. La ruta de deshacer una operación
de selección continúa invalidando normalmente porque solo restaura el buffer,
sin calcular previamente el compuesto. Se conserva la subida completa cuando
hay cambios regionales posteriores a un historial aún pendiente de subir.

Dos pruebas de integración comprueban la validez inmediata del compuesto y
la identidad del `Arc` enviado. Al retener otra referencia, cualquier escritura
innecesaria forzaría COW y fallaría la comparación de identidad; no se depende
del reloj para probar la eliminación del trabajo. Se cubren deshacer/rehacer,
saltos, restauraciones consecutivas sin render intermedio, cambios de tamaño y
ediciones antes de consumir la subida.

Se repite la sesión de ocho escenas descrita en
[interaction-validation.md](interaction-validation.md), preservando el informe
y los resultados anteriores como línea base. Los resultados nuevos quedan en
[interaction-history-results.txt](interaction-history-results.txt).

Percentil 95 de acción más preparación de subida, en milisegundos. Cada celda
muestra la línea base anterior y la ejecución posterior con la corrección:

| Escena | Deshacer anterior → actual | Rehacer anterior → actual |
| --- | ---: | ---: |
| 1920 × 1080, una capa | 2,688 → 1,192 | 20,130 → 9,768 |
| 1920 × 1080, activa arriba | 4,910 → 2,566 | 117,342 → 58,836 |
| 1920 × 1080, activa debajo | 586,196 → 328,281 | 611,968 → 383,294 |
| 1920 × 1080, grupos anidados | 126,108 → 64,246 | 269,783 → 120,772 |
| 6000 × 4000, una capa | 25,275 → 15,188 | 91,980 → 49,789 |
| 6000 × 4000, activa arriba | 1215,761 → 536,869 | 1437,293 → 640,688 |
| 6000 × 4000, activa debajo | 1707,044 → 774,614 | 1840,797 → 942,220 |
| 6000 × 4000, grupos anidados | 1327,797 → 638,738 | 1453,737 → 781,050 |

En la escena grande con activa arriba, las medianas pasan de 1175,565 a
524,062 ms al deshacer y de 1302,989 a 601,598 ms al rehacer. Preparar la subida
después de restaurar tarda como máximo unos 0,001 ms en estas muestras, en vez
de volver a componer. Quedan la restauración y una composición completa en el
tiempo de la acción; los casos detallados siguen pudiendo causar pausas.

Son tres pares por escena, al deshacer y rehacer el último cambio de visibilidad
de la sesión. El percentil 95 equivale al máximo de esas tres muestras; no es
una garantía estadística ni una medición GPU o de latencia de pantalla. Las
pruebas de integración también cubren historial con pintura y redimensionado.
Las ocho sesiones pasan sus comprobaciones de píxeles y referencia completa.

Verificación: `cargo test --workspace --offline` pasa 199 pruebas y deja una
de portapapeles ignorada. Pasan `cargo fmt --all -- --check` y `git diff --check`.

## Validación de sesiones del editor

Se añade `interaction_session_bench`: ocho sesiones con movimientos reales de
las herramientas, preparación de subida por evento, capas e historial, en
1920 × 1080 y 6000 × 4000. Registra percentiles y comprueba cancelación,
deshacer/rehacer y la imagen final contra la referencia. No modifica los
algoritmos de producción.

El primer candidato detectado es la doble composición al deshacer/rehacer.
También quedan pausas en curvas bajo varias capas detalladas y en cambios
completos de opacidad/visibilidad. El informe distingue estas mediciones CPU
de la comprobación de la ventana nativa, limitada por un fallo del controlador
de entrada: [interaction-validation.md](interaction-validation.md).

## Decimotercera etapa: trabajo caro sin fondo estático detallado

Se añade una segunda señal para admitir la composición paralela: el coste
observado en las filas calculadas durante la actualización actual. Resuelve
casos en los que las capas detalladas están por encima del activo y el fondo
cacheable es plano, o donde sus firmas cambian y no hay evidencia estática
suficiente para las reglas anteriores.

El cronómetro solo se activa para regiones de más de 8 388 608 píxeles, el
equivalente a 32 MiB RGBA. Al completar una fila de temporales se exige haber
calculado al menos 1 048 576 píxeles, que queden más de 8 388 608 y que el coste
observado sea de al menos 50 ms por cada 1 048 576 píxeles. Se normaliza por el
tamaño de la muestra: acumular tiempo en un recorrido barato no basta.
En ese umbral, el coste serial estimado del resto supera unos 400 ms.

No se guardan tiempos entre actualizaciones, por lo que un dibujo anterior
caro no fuerza la estrategia de uno posterior barato. La decisión conserva
las filas ya compuestas y la caché; solo distribuye las filas restantes con
el compositor de referencia y los mismos hasta ocho hilos de etapas previas.
El límite de caché sigue siendo 32 MiB de píxeles y 8192 entradas; temporales,
pilas y metadatos se cuentan aparte. La llamada sigue esperando a los hilos.

### Medición: capa activa debajo de seis capas detalladas

`cargo run --release -p rspaint-core --example full_composite_cache_bench --offline -- --active-below-detail`
mantiene ocho capas de 6000 × 4000: el activo está encima del fondo blanco y
debajo de seis capas estáticas. Estas seis capas deben mezclarse contra el
resultado del activo, por lo que no se pueden aplanar juntas sobre transparencia
y reutilizar como si fueran un fondo independiente.

| Contenido | Primera composición anterior → actual | Mediana de tres cambios de opacidad anterior → actual |
| --- | ---: | ---: |
| Plano | 345,687 → 261,089 ms | 277,859 → 213,747 ms |
| Poco pintado | 237,343 → 203,511 ms | 167,054 → 149,548 ms |
| Muy detallado | 3900,141 → 782,475 ms | 3125,914 → 672,277 ms |

El detallado mejora unas 5 veces al inicio y 4,6 veces en actualizaciones,
con ocho hilos. Conserva solo 1504 bytes de píxeles comprimidos del fondo
en 376 entradas. Los casos plano y poco pintado usan cero hilos directos;
sus diferencias entre ejecuciones no se atribuyen a la nueva vía paralela.
Otra ejecución con la misma estrategia dio 791,667 ms iniciales y 686,522 ms
de mediana posterior para el detallado. Son mediciones locales, no garantías.

El benchmark compara la imagen completa con la referencia inmediatamente
después de la primera composición y tras los cambios de opacidad; los tres
contenidos pasan ambas comparaciones. Como control, el escenario original
con el activo arriba conserva la ruta comprimida en los casos plano y poco
pintado (65,489 y 61,067 ms de mediana), y el detallado da 613,372 ms.

La nueva prueba de admisión usa duraciones suministradas, sin esperas reales:
comprueba el límite de área, el mínimo de muestra, el umbral de 49/50 ms y
que un tiempo total alto sobre muchos píxeles baratos no active la estrategia.
Las pruebas existentes de franjas verifican grupos, todos los modos, Disolver,
recortes y conservación de copias compartidas.

El tiempo medido es de pared: una pausa del sistema puede sobreestimar el
coste, y un prefijo caro puede preceder a una región barata. Esto solo cambia
la estrategia, conservando el resultado exacto. En cada actualización aún se
calcula un prefijo serial antes de decidir. Quedan fuera de esta regla las
regiones de hasta 32 MiB y las cargas por debajo del umbral; optimizar su
cálculo o medir otras herramientas sigue siendo trabajo independiente.

Verificación: `cargo test --workspace --offline` pasa 197 pruebas y deja una
de portapapeles ignorada. Pasan `cargo fmt --all -- --check` y `git diff --check`.

## Duodécima etapa: admitir fondos detallados dentro de grupos

La decisión de recorrer una región grande en paralelo consulta ahora las
cachés internas de los grupos visibles, además de la caché raíz. Recorre los
grupos que el compositor calcula por separado, incluidos los anidados; las
subramas que ya forman parte del fondo estático se evalúan en ese fondo.
Los grupos ocultos o con opacidad cero no aportan evidencia interna.

Cada ámbito exige sus propios bloques actuales: al menos 4 MiB decodificados,
90 % de sus bytes correspondientes a la firma vigente y una estimación que
supere 1,25 veces el presupuesto para la región solicitada. El denominador del
90 % es ahora el ámbito evaluado; una caché plana de otro grupo no invalida
la evidencia de uno detallado. Un cambio de fuente invalida su firma. Cambiar
la opacidad del grupo conserva la validez de sus píxeles interiores, que se
componen antes de aplicar esa opacidad.

No cambia la mezcla: se reutiliza el compositor de referencia para las franjas
paralelas. Se mantienen hasta ocho hilos, 32 MiB de píxeles retenidos y 8192
entradas, con los temporales y metadatos descritos en las etapas anteriores.
Las consultas locales de hasta 32 MiB siguen usando la ruta de caché.

### Medición con grupo aislado

`cargo run --release -p rspaint-core --example full_composite_cache_bench --offline -- --group`
encierra las ocho capas de 6000 × 4000 en un grupo con opacidad 73 %. El árbol
no tiene un fondo raíz independiente del activo, por lo que antes no admitía
la ruta paralela. Se cambia la opacidad de la capa activa dentro del grupo.
La referencia completa verifica tanto el resultado inicial como el posterior.

| Contenido | Primera composición anterior → actual | Mediana de tres actualizaciones anterior → actual |
| --- | ---: | ---: |
| Plano | 453,248 → 348,061 ms | 135,224 → 149,665 ms |
| Poco pintado | 319,355 → 308,903 ms | 108,496 → 120,353 ms |
| Muy detallado | 4283,444 → 961,358 ms | 3821,706 → 623,803 ms |

El caso detallado mejora unas 4,5 veces al inicio y 6,1 veces en actualizaciones.
Usa ocho hilos y conserva 6 144 000 bytes en 376 entradas. Los casos plano y
poco pintado conservan la ruta comprimida con cero hilos directos. Sus cambios
de tiempo no constituyen una mejora medida; son ejecuciones locales de pocas
muestras, con costes adicionales de consultar los ámbitos del árbol.

Una prueba específica cubre dos grupos anidados, regiones pequeñas, visibilidad,
opacidad cero, cambios de opacidad exterior y cambio del fondo detallado a
plano. Las pruebas de franjas existentes comparan todos los modos, Disolver,
grupos y recortes de columnas contra la referencia.

La política sigue siendo una estimación basada en bloques ya calculados.
No detecta el trabajo caro de capas dinámicas cuando no existe suficiente
evidencia estática cacheable; tampoco suma todavía varios ámbitos pequeños
que individualmente no alcanzan el umbral de admisión.

Verificación: `cargo test --workspace --offline` pasa 196 pruebas y deja una
de portapapeles ignorada. Pasan `cargo fmt --all -- --check` y `git diff --check`.

## Undécima etapa: primera composición detallada

El compositor evalúa la compresión de los bloques ya calculados al terminar
cada fila de temporales. Ahora puede decidir un recorrido directo después de
observar al menos 4 MiB de fondo raíz válido (un octavo del presupuesto), sin
esperar a llenar la caché. Se mantienen la exigencia de que el 90 % de sus bytes
pertenezcan al fondo actual y la estimación de más de 1,25 presupuestos para la
región pendiente. Las consultas de hasta 32 MiB siguen usando la ruta de caché.

Si se admite el cambio, solo se procesan en paralelo las filas restantes:
los píxeles y bloques ya calculados se conservan. El primer bloque detallado
no exige una pasada de muestreo adicional ni recomponer la imagen completa.
Se reutilizan los hilos con préstamos acotados de la etapa anterior, hasta ocho;
no cambia el límite de 32 MiB de píxeles almacenados ni el límite de entradas.

### Medición y exactitud

`cargo run --release -p rspaint-core --example full_composite_cache_bench --offline`
comprueba ahora también toda la imagen inmediatamente después de la primera
composición, antes de que otra actualización pueda ocultar un error en la unión
de las filas calculadas con caché y las filas calculadas en paralelo.

| 6000 × 4000, ocho capas | Primera composición | Mediana de tres cambios de opacidad | Hilos directos |
| --- | ---: | ---: | ---: |
| Plano | 407,677 ms | 115,729 ms | 0 |
| Poco pintado | 371,273 ms | 96,749 ms | 0 |
| Muy detallado | 1115,526 ms | 623,735 ms | 8 |

La primera composición detallada pasó de 4671,614 ms en la etapa anterior a
1115,526 ms: aproximadamente 4,2 veces más rápida. Una ejecución exploratoria
de esta etapa dio 1179,780 ms. Los tiempos son mediciones locales, no garantías
para otros equipos. Los casos planos continúan comprimidos; sus cifras varían
respecto a la ejecución anterior y no se atribuye una mejora en esos casos.
La caché detallada conserva 6 144 000 bytes en 376 entradas (unos 5,9 MiB).
Las comparaciones completas iniciales y posteriores pasan en los tres casos.

La prueba de admisión comprueba que una muestra de un octavo del presupuesto
permite decidir aun con la caché incompleta y que una muestra menor no basta.
Siguen cubiertos cambios de firma, contenido que pasa a ser plano y consultas
locales. La estimación usa el prefijo observado: un documento con detalle solo
al principio podría tomar una decisión menos rentable para el resto, aunque
el resultado de píxeles sigue siendo exacto. No se paralelizan todavía todas
las topologías de grupos; la evidencia requerida sigue siendo del fondo raíz.

Verificación: `cargo test --workspace --offline` pasa 195 pruebas y deja una
de portapapeles ignorada por requerir interacción. Pasan también
`cargo fmt --all -- --check` y `git diff --check`.

## Décima etapa: composición de imágenes detalladas

La expulsión de bloques usa un índice LRU ordenado: seleccionar y actualizar
la antigüedad cuesta O(log n), evitando recorrer todo el mapa por expulsión.
Continúan los límites de 8192 entradas y 32 MiB de píxeles almacenados;
el índice añade metadatos acotados fuera de ese presupuesto de píxeles.

La mezcla Normal sobre un destino opaco tiene una especialización que conserva
la aritmética y el redondeo de la referencia. Una tabla contiene los mismos
256 valores f32 normalizados; solo se eliminan términos multiplicados por cero
o uno. Las pruebas comparan los bits de la tabla y aproximadamente 98 mil
combinaciones de canales, alfa y opacidad con `blend_pixel`, que sigue siendo
la referencia. Otros modos y destinos translúcidos conservan la ruta general.

Cuando una región supera el presupuesto de caché y los bloques detallados no
caben, el compositor puede calcular franjas independientes con hasta ocho
hilos disponibles. La decisión exige una caché al menos 90 % llena, que al menos
90 % de sus bytes corresponda a la firma actual del fondo raíz y una estimación
de tamaño superior a 1,25 veces el presupuesto. Las regiones pequeñas siguen
usando la caché; un cambio del fondo invalida esta evidencia y permite aprender
de nuevo si el contenido pasó a ser comprimible.

Los hilos toman prestadas las fuentes inmutables y escriben franjas disjuntas,
sin `unsafe` ni dependencias nuevas. Se conserva la caché para consultas locales.
Los temporales de ocho hilos pueden sumar hasta 2 MiB por profundidad de grupo,
además de pilas y metadatos; el límite de 32 MiB no es un límite de RAM total.
La llamada espera a que terminen los hilos: no es composición asíncrona.

### Mediciones CPU

`cargo run --release -p rspaint-core --example full_composite_cache_bench --offline`
usa 6000 × 4000, ocho capas y tres cambios de opacidad. Cada resultado completo
se compara byte por byte con el compositor de referencia sin caché.

| Contenido | Primera composición | Mediana de cambios de opacidad | Hilos directos |
| --- | ---: | ---: | ---: |
| Plano | 373,181 ms | 72,463 ms | 0 |
| Poco pintado | 276,503 ms | 78,268 ms | 0 |
| Muy detallado | 4671,614 ms | 681,271 ms | 8 |

En el caso detallado, la mediana anterior era 5933,205 ms: la actualización
posterior mejora aproximadamente 8,7 veces. La caché almacena 33 544 192 bytes,
por debajo de 32 MiB. Los casos plano y poco pintado conservan la ruta comprimida.
La primera composición detallada sigue siendo costosa porque aún no existe
evidencia de caché para admitir el recorrido paralelo.

### Pipeline GPU

`cargo run --release -p rspaint-app --example gpu_pipeline_bench --offline -- --case=detailed-opacity-8`
usa el callback real, siete muestras medidas tras dos de calentamiento y una
RTX 3060 Ti, Vulkan, controlador 621.01.

| Documento | CPU composición/preparación | Encolado de subida | Envío hasta finalización | Dibujo GPU | Pipeline completo |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1920 × 1080 | 57,654 ms | 0,995 ms | 0,994 ms | 0,029 ms | 60,391 ms |
| 6000 × 4000 | 719,557 ms | 9,378 ms | 34,944 ms | 1,951 ms | 765,415 ms |

Son medianas independientes; sus columnas no deben sumarse. El percentil 95
del pipeline grande fue 837,823 ms. Este escenario tiene también detalle en la
capa activa, mientras el ensayo CPU usa una capa activa uniforme; no son la
misma carga. No hay una medición GPU anterior de este escenario para atribuir
una mejora del pipeline. Se excluyen eventos de ventana, vsync, presentación
en pantalla y confirmación del historial.

### Verificación y límites

Las pruebas cubren franjas con columnas recortadas, grupos, los trece modos,
coordenadas globales de Disolver y conservación de copias compartidas. También
comprueban la consistencia del índice LRU y la selección conservadora de ruta.
`cargo test --workspace --offline`: 195 pruebas aprobadas y una de portapapeles
ignorada por requerir interacción. El ejemplo GPU pasa sus tres pruebas;
`cargo fmt --all -- --check` y `git diff --check` también pasan.
La política no paraleliza todas las topologías: requiere evidencia válida del
fondo raíz. Queda como siguiente foco la primera composición de imágenes
detalladas y las topologías que no cumplen ese criterio.

## Novena etapa: compresión sin pérdida y mezcla de regiones uniformes

La caché de composición conserva los bloques planos como un solo color RGBA.
Cuando hay pocos cambios de color, puede usar secuencias de color y longitud
(RLE); si no ahorra espacio, conserva los píxeles crudos. Una muestra de hasta
64 píxeles descarta temprano la codificación RLE en bloques muy detallados.
Todo se reconstruye byte por byte, incluidos RGB con alfa cero.

El presupuesto de píxeles almacenados continúa en 32 MiB. El límite de entradas
pasa de 2048 a 8192, para que un documento grande pueda conservar miles de
bloques pequeños comprimidos; esto permite más metadatos y no equivale a un
límite de 32 MiB de RAM total. Las firmas, mapas y objetos de caché se cuentan
aparte. Los bloques decodificados temporales siguen siendo pequeños, y las
imágenes detalladas no reciben una caché de píxeles ilimitada.

Además, cuando una región de origen y destino es completamente uniforme, el
compositor calcula exactamente la misma mezcla una vez y rellena la región.
La detección compara todos sus píxeles y conserva el cálculo y redondeo de
`blend_pixel`. La vía rápida exige que el origen cubra todo el destino y no
se aplica a Disolver, cuyo resultado depende de la posición. Las regiones
variables conservan el recorrido por píxeles.

### CPU, imagen completa y verificación exacta

```sh
cargo run --release -p rspaint-core --example full_composite_cache_bench --offline
```

Ocho capas, lienzo 6000 × 4000, tres cambios de opacidad de la capa superior
después de componer inicialmente. Las seis capas estáticas adicionales comparten
una imagen de origen para acotar la RAM del generador. Cada resultado final se
compara con la composición sin caché de los 24000000 píxeles. El caso «detallado»
usa ruido RGB determinista con alfa 140; es una prueba adversa de compresibilidad,
no un conjunto representativo de fotografías.

| Contenido estático | Primera composición | Opacidad mediana / máximo | Píxeles almacenados | Equivalente sin comprimir | Entradas |
| --- | ---: | ---: | ---: | ---: | ---: |
| Plano | 394,217 ms | 71,252 / 72,180 ms | 23688 B | 96000000 B | 5922 |
| Poco pintado | 318,323 ms | 87,482 / 88,908 ms | 28776 B | 96000000 B | 5922 |
| Muy detallado | 5816,265 ms | 5933,205 / 6004,659 ms | 33544192 B | 33544192 B | 2100 |

Los tres casos pasan la comparación exacta de imagen completa. El presupuesto
se comprueba también dentro del benchmark. Para el caso detallado, una pasada
de referencia sin caché y su comparación tardaron 5665,434 ms en la misma
ejecución: la compresión no mejora ese escenario y queda un pequeño costo de
gestión de caché. Sus bloques siguen superando el presupuesto y se expulsan.
La primera medición, con solo compresión y sin la vía uniforme, había reducido
el caso plano a unos 712 ms; la vía uniforme reduce el trabajo restante.

### Renderizador real fuera de pantalla

```sh
cargo run --release -p rspaint-app --example gpu_pipeline_bench --offline -- --case=opacity-8
```

Mismo escenario validado de la etapa anterior: RTX 3060 Ti/Vulkan, ocho capas,
opacidad de la capa seleccionada, salida de 1024 × 768, dos actualizaciones de
calentamiento y siete muestras. Las exclusiones de ventana, vsync y pantalla
siguen vigentes. El tiempo completo termina al completarse el envío de GPU;
no es latencia física de puntero a pantalla.

| Lienzo | CPU antes | CPU actual | Camino CPU → GPU antes | Actual mediana / p95 |
| --- | ---: | ---: | ---: | ---: |
| 1920 × 1080 | 63,718 ms | 14,658 ms | 67,223 ms | 17,101 / 19,213 ms |
| 6000 × 4000 | 4878,110 ms | 71,025 ms | 4934,497 ms | 89,104 / 97,576 ms |

Son ejecuciones separadas en el mismo entorno, sujetas a variación. La mejora
del camino completo del caso grande plano es de aproximadamente 55×. La caché
almacena 23688 bytes de colores para un compuesto equivalente a 96 MB; los
metadatos de sus 5922 entradas no están incluidos en esos bytes. No se ha medido
RAM total ni VRAM total. El pase GPU varía con el estado de energía y no se
atribuye su variación a esta optimización de CPU.

### Verificación y próximo foco

`cargo test --workspace --offline`: 192 pruebas correctas y una prueba de
portapapeles omitida según la configuración del proyecto. Se añadieron pruebas
de ida/vuelta RGBA para colores planos, secuencias y datos detallados; límites
de bytes y de entradas pequeñas comprimidas; y comparación de la vía uniforme
con el cálculo por píxel en los 13 modos, opacidades, transparencia, recortes
y regiones no uniformes. Las pruebas existentes cubren invalidación, historial,
selección, grupos, bordes y ramas de edición.

El caso plano/poco pintado mejora mucho, pero no implica que todos los lienzos
grandes se actualicen en tiempo real. El siguiente foco son los compuestos muy
detallados que no caben ni comprimidos: evaluar una política de barridos completos
y el costo del recorrido de mezcla, manteniendo el límite de memoria y la
equivalencia de píxeles. Las subidas completas de unos 92 MiB también siguen
siendo un costo independiente.

## Octava etapa: escenarios exigentes y renderizador real con GPU

### Regresión detectada y corregida

Los cambios repetidos en una capa de la pila estática invalidaban la caché y
expandían una región pequeña a bloques alineados completos en cada solicitud.
Con 32 capas, la región de 64 × 48 tardaba 13,178 ms en 1920 × 1080 y 14,075 ms
en 6000 × 4000, frente a unos 2,4–2,8 ms del recorrido directo.

Ahora cada pila/grupo recuerda su firma entre actualizaciones. Si cambia,
compone directamente la región solicitada durante toda esa actualización,
incluidos todos sus fragmentos de 256 × 256. Al estabilizarse, vuelve a llenar
y reutilizar la caché. Las firmas se calculan una vez por pila y fragmento,
en lugar de reconstruirlas para cada bloque de caché de 64 × 64. No se aumentó
el límite de 32 MiB de píxeles ni el de 2048 entradas; las firmas y sus estados
son metadatos adicionales, también con cantidad acotada.

```sh
cargo run --release -p rspaint-core --example composition_stress_bench --offline
```

Medianas de 21 muestras, región 64 × 48, verificando cada resultado contra el
compositor de referencia. La capa activa inicial es blanca; la pila estática
combina Normal, Multiplicar, Pantalla y Superponer. El caso de opacidad modifica
programáticamente una capa estática debajo de la activa, no el control normal
de opacidad de la capa seleccionada. El caso anidado tiene ocho niveles, con
opacidad y modos de mezcla de grupo. Las capas uniformes comparten almacenamiento
para acotar la RAM del escenario, pero participan todas en la composición.

| Lienzo | Escenario | Referencia | Caché actual | p95 caché |
| --- | --- | ---: | ---: | ---: |
| 1920 × 1080 | Activa arriba, 32 capas | 2,628 ms | 0,013 ms | 0,020 ms |
| 1920 × 1080 | Activa abajo, 32 capas | 2,701 ms | 2,713 ms | 2,770 ms |
| 1920 × 1080 | Ocho grupos anidados | 1,528 ms | 1,366 ms | 1,406 ms |
| 1920 × 1080 | Opacidad estática cambiante, 32 capas | 2,662 ms | 2,693 ms | 2,741 ms |
| 6000 × 4000 | Activa arriba, 32 capas | 2,704 ms | 0,022 ms | 0,040 ms |
| 6000 × 4000 | Activa abajo, 32 capas | 2,775 ms | 2,782 ms | 2,944 ms |
| 6000 × 4000 | Ocho grupos anidados | 1,664 ms | 1,523 ms | 1,703 ms |
| 6000 × 4000 | Opacidad estática cambiante, 32 capas | 2,785 ms | 2,821 ms | 2,935 ms |

### Medición del camino CPU → GPU

```sh
cargo run --release -p rspaint-app --example gpu_pipeline_bench --offline
# Un caso específico:
cargo run --release -p rspaint-app --example gpu_pipeline_bench --offline -- --case=opacity-8
```

El ejemplo importa el `CanvasRenderer` y el callback usados por la aplicación.
Renderiza a una textura de 1024 × 768, RGBA8 sin sRGB, con fondo, cuadrícula y
cursor habilitados. Dispositivo medido: NVIDIA GeForce RTX 3060 Ti, Vulkan,
controlador NVIDIA 621.01. `TIMESTAMP_QUERY` mide el pase de dibujo GPU, incluido
el trabajo del render pass; no mide la subida de textura. Los tiempos de CPU,
encolado, envío y espera se miden con reloj de pared y se imprimen por separado.
`pollster`, ya presente transitivamente, se agregó solo como dependencia de
desarrollo para ejecutar este benchmark asíncrono.

Se consume el upload inicial y se omiten dos actualizaciones de calentamiento.
Se miden siete movimientos de un trazo en curso, sin confirmar historial.
«Parcial» conserva una región pequeña; «completa» fuerza un upload del lienzo
entero. El escenario de opacidad tiene ocho capas, selecciona la capa superior
y cambia su opacidad como hace el control de la interfaz. Las otras capas
uniformes comparten memoria de origen. El generador valida IDs únicos, número
de capas y capa activa; una prueba adicional protege estas condiciones.

| Lienzo | Escenario | Composición/preparación CPU | Pase GPU | Camino completo mediana / p95 |
| --- | --- | ---: | ---: | ---: |
| 1920 × 1080 | Parcial, una capa | 0,015 ms | 0,026 ms | 0,319 / 0,702 ms |
| 1920 × 1080 | Parcial, 31 capas superiores | 2,770 ms | 0,027 ms | 3,237 / 3,547 ms |
| 1920 × 1080 | Completa, una capa | 4,245 ms | 0,026 ms | 6,423 / 7,272 ms |
| 1920 × 1080 | Opacidad de la activa, ocho capas | 63,718 ms | 0,287 ms | 67,223 / 70,976 ms |
| 6000 × 4000 | Parcial, una capa | 0,013 ms | 0,764 ms | 1,353 / 2,907 ms |
| 6000 × 4000 | Parcial, 31 capas superiores | 2,753 ms | 0,778 ms | 4,646 / 5,895 ms |
| 6000 × 4000 | Completa, una capa | 50,153 ms | 0,760 ms | 84,011 / 89,326 ms |
| 6000 × 4000 | Opacidad de la activa, ocho capas | 4878,110 ms | 1,872 ms | 4934,497 / 5093,054 ms |

En la subida completa de 6000 × 4000 se envían 96000000 bytes (91,553 MiB).
El encolado de esa subida midió 10,920 ms de mediana y el envío hasta terminar
24,247 ms en el caso de una capa. Las medianas de etapas distintas no tienen
por qué sumar la mediana total. La instrumentación añade resolución/copia de
dos timestamps y una espera explícita hasta terminar cada envío.

Estas son mediciones fuera de pantalla, no latencia física de puntero a pantalla:
excluyen el planificador de la ventana, el resto de la UI, compositor de escritorio,
vsync y panel. Tampoco miden RAM total, memoria dedicada de GPU ni documentos con
32 asignaciones independientes de origen. Los percentiles de siete muestras son
orientativos y dependen de la carga del sistema y del estado de energía del GPU.
Las cifras publicadas son del generador validado; se descartaron las primeras
ejecuciones que contenían IDs duplicados en la preparación del escenario.

### Prioridad que dejan las mediciones

La composición completa en CPU y las subidas completas fueron el siguiente foco
de la novena etapa.
Con 1920 × 1080, la pila inferior ocupa unos 8 MiB y cabe en caché; con
6000 × 4000 ocupa unos 92 MiB y supera los 32 MiB/2048 bloques de la caché.
Un barrido completo repetido no conserva toda esa pila entre actualizaciones,
por lo que la opacidad de un documento grande sigue siendo un caso costoso.
La GPU dibuja el compuesto ya aplanado; su pase corto no evita ese trabajo CPU.
Conviene evaluar una política específica para actualizaciones completas y medir
su intercambio entre memoria y tiempo antes de cambiar el presupuesto de caché.

Verificación: 160 pruebas de core/editor y 29 de la aplicación correctas;
una prueba de portapapeles está marcada como omitida por el proyecto. El harness
de GPU pasa sus tres pruebas (incluidas dos del callback compartido). Los
benchmarks CPU verifican igualdad exacta de píxeles. La prueba nueva de caché
cubre invalidación repetida, reanudación tras estabilizar y varios fragmentos
dentro de la misma actualización, sobre una capa activa semitransparente.

## Séptima etapa: pinceles grandes en modo mezcla

`draw_line_masked` usa ahora la unión de huellas por intervalos de fila también
para pinceles cuadrados y circulares en modo mezcla, desde 8 px y con movimiento.
La máscara ya definía una sola aplicación de color por píxel y trazo, así que
agrupar las huellas no cambia la acumulación de alfa. Se conserva `Color::over`
para los píxeles nuevos y el mismo recorte de selección. Las puntas pequeñas,
diagonales y aplicaciones estacionarias mantienen el recorrido de estampado.
`draw_line` sin máscara conserva su comportamiento de mezcla acumulativa.

La construcción de intervalos cuesta aproximadamente longitud × altura de la
punta, más preparar su geometría, en vez de visitar su área completa en cada
posición. Después solo se recorren los píxeles cubiertos por la unión. Los
intervalos totalmente consumidos por la máscara no solicitan acceso mutable al
buffer. Un color transparente en modo mezcla consume la máscara sin escribir
píxeles; el modo reemplazo sigue escribiendo también el alfa cero.

```sh
cargo run --release -p rspaint-core --example blend_brush_bench --offline
```

Medianas de tres muestras de CPU en 1920 × 1080, con un segmento entre (300,300)
y (1300,800), alfa de pincel 96 sobre fondo con alfa 150 y una máscara que excluye
uno de cada diecisiete píxeles. El estampado de referencia reconstruye el
recorrido anterior usando la misma punta, máscara y cálculo de color. Cada
muestra comprueba igualdad exacta de los píxeles y de la máscara final. La copia
inicial por escritura y la creación de máscaras ocurren fuera del tiempo medido.
No incluye respaldo de previsualización, composición de capas ni GPU.

| Punta | Tamaño | Estampado | Intervalos | Mejora |
| --- | ---: | ---: | ---: | ---: |
| Cuadrada | 8 px | 0,654 ms | 0,440 ms | 1,5× |
| Cuadrada | 64 px | 10,762 ms | 2,806 ms | 3,8× |
| Cuadrada | 256 px | 114,978 ms | 9,738 ms | 11,8× |
| Circular | 8 px | 0,566 ms | 0,429 ms | 1,3× |
| Circular | 64 px | 13,670 ms | 1,959 ms | 7,0× |
| Circular | 256 px | 206,007 ms | 9,003 ms | 22,9× |

Verificación actual: 159 pruebas correctas entre core y editor;
`cargo check -p rspaint-app --offline`, formato y revisión de espacios correctos.
Las pruebas comparan ambas rutas con puntas cuadradas, circulares y diagonales,
tamaños a ambos lados del umbral, alfa 0/1/96/254/255, fondos con alfa variable,
selecciones con huecos, máscaras truncadas, recorridos superpuestos o invertidos,
bordes, posiciones exteriores y aplicaciones estacionarias. También verifican
que la ruta de mezcla transparente o totalmente cubierta no separa un buffer
compartido y que se conserva la ruta anterior para un stride de máscara distinto
al ancho real del buffer.

La composición exigente y el camino CPU → GPU se midieron en la octava etapa.
La latencia de ventana/pantalla y la memoria total siguen pendientes. Persisten
además respaldos completos en algunas operaciones de selección y transformación.

## Sexta etapa: respaldos regionales para trazos y previsualizaciones

Los trazos libres, las formas, las curvas y el texto usan ahora `PreviewBackup`:
capturan los píxeles originales de cada bloque de 128 × 128 justo antes de
tocarlo por primera vez. Al crecer o mover una previsualización se agregan
solo bloques nuevos; se conservan los originales de los bloques ya capturados.
Cancelar restaura esos bloques y el modo píxel perfecto consulta el original
del píxel que necesita recuperar. El texto también restaura y sube regiones,
en vez de restaurar y recomponer todo el lienzo en cada tecla.

El buffer de trabajo antes compartía sus píxeles con la entrada de la capa
activa del árbol. Durante una edición interactiva `prepare_active_edit` libera
ese espejo obsoleto y deja una entrada de marcador temporal. El buffer de
trabajo es la fuente autoritativa para composición y miniaturas activas.
`sync_active` repone la entrada completa antes de snapshots, cambios de capa
y operaciones sobre el árbol; `refresh_buffer` conserva el buffer si la capa
activa está editándose. Los consumidores que copien directamente el árbol
deben sincronizarlo antes; esto queda documentado en `Document`.

Las copias externas genuinas conservan el comportamiento copy-on-write:
si una selección, una transformación u otro consumidor retiene una imagen
completa, todavía puede ser necesaria una copia completa al escribir.
Tampoco se eliminó el respaldo completo de selecciones y transformaciones.
El primer dibujo antes de mostrar el lienzo puede compartir todavía el buffer
con el compuesto inicial; el benchmark consume primero ese upload inicial.

```sh
cargo run --release -p rspaint-editor --example preview_backup_bench --offline
```

Medianas de siete muestras de CPU, perfil release. Las regiones están cerca
de (127,127), cruzan bordes de bloques y el texto escribe «Hola». Cancelar ocurre
fuera del tiempo medido. Las muestras conservan la misma asignación de píxeles
del buffer de trabajo, comprobada por el benchmark.

| Lienzo | Copia compartida reconstruida* | Inicio lápiz | Inicio línea | Inicio y movimiento de curva | Apertura y preview de texto |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1920 × 1080 | 1,538 ms | 0,019 ms | 0,020 ms | 0,022 ms | 0,057 ms |
| 6000 × 4000 | 17,046 ms | 0,026 ms | 0,026 ms | 0,029 ms | 0,081 ms |

*Se reconstruye únicamente la primera escritura de un buffer compartido.
No es una ejecución de la implementación anterior completa ni una medición
de latencia de la aplicación con GPU. Los tiempos actuales incluyen la captura
regional, las máscaras necesarias y el dibujo de cada caso. Un bloque ocupa
hasta 64 KiB; una región pequeña que cruza ambos bordes puede capturar cuatro
bloques (256 KiB), frente a 96 MB de una copia completa de 6000 × 4000.
Si la operación acaba cubriendo todo el lienzo, el respaldo también puede crecer
hasta ese tamaño, más su tabla de bloques. Las regiones se calculan con cajas
envolventes conservadoras, así que una diagonal larga puede capturar más área
que los píxeles realmente pintados.

Verificación actual: 157 pruebas correctas entre core y editor y compilación
de la aplicación. Se comprueban cancelación exacta, previsualizaciones que se
encogen o cruzan bloques, transparencia, selección, modo píxel perfecto,
confirmación y undo/redo de curvas y texto, sincronización de capas y aislamiento
de snapshots externos. La asignación continua se conserva durante las pruebas
de lápiz, pincel, borrador, línea, rectángulo, elipse, rectángulo redondeado,
curvas y texto cuando no hay otro propietario externo de los píxeles.

Los pinceles grandes en modo mezcla se optimizaron en la séptima etapa.

## Quinta etapa: detectar bloques modificados sin recorrer todos los píxeles

`PixelBuffer` mantiene una tabla compartida de versiones por bloque de 128 × 128.
Al escribir se actualizan únicamente las versiones de los bloques afectados.
Si la tabla está compartida con una instantánea u otro buffer, se separa y se
asigna una generación única a la nueva tanda de escrituras. Esto evita confundir
ediciones independientes de dos clones. La generación no requiere una operación
atómica por píxel; se obtiene al crear el buffer o separar una tabla compartida.

Al confirmar una acción se recorre la tabla de bloques y se reutilizan directamente
los de versión idéntica. Solo los candidatos modificados comparan bytes antes de
decidir si necesitan una nueva copia. Así también se conserva el ahorro para
escrituras sin cambio efectivo de color. Reconstruir una instantánea restaura sus
versiones; las versiones no forman parte de la igualdad de imágenes.

Se cubrieron `set`/`paint`, copias de regiones, `blit` y rellenos. La nueva API
`row_mut` devuelve exclusivamente los bytes de un tramo de fila y marca sus
bloques; la usan el borrador por intervalos y el pincel en modo reemplazo.
`data_mut` sigue permitiendo escritura arbitraria y marca todos los bloques:
su siguiente instantánea conserva la comprobación completa por seguridad.
Los constructores y transformaciones que crean imágenes nuevas parten de
generaciones diferentes, incluso cuando conservan las dimensiones originales.

Última ejecución del benchmark de historial, mediana de doce cambios de un píxel,
en CPU y perfil release:

| Lienzo | Confirmar antes | Confirmar ahora | Máximo actual | Primera escritura compartida actual |
| --- | ---: | ---: | ---: | ---: |
| 2048 × 2048 | 4,139 ms | 0,958 ms | 1,118 ms | 3,381 ms |
| 6000 × 4000 | 26,321 ms | 5,181 ms | 5,327 ms | 16,963 ms |

Son ejecuciones separadas, sujetas a carga y variación; otra ejecución de esta
etapa midió medianas de 0,749 y 4,219 ms al confirmar. El tiempo incluye la
sincronización del buffer y liberación de imágenes anteriores, además de crear
la instantánea. Sigue habiendo un recorrido barato de la tabla de bloques,
no se promete costo constante independiente del tamaño de imagen.

El RGBA retenido por el historial no cambia: 16,750 y 92,303 MiB después de doce
ediciones, respectivamente. Las tablas de versiones añaden ocho bytes por bloque
(unos 12 KiB para 6000 × 4000), compartidos mientras no se editen. Estas tablas
y otros metadatos quedan fuera de la métrica RGBA del benchmark.

Verificación actual: 153 pruebas correctas entre core y editor y
`cargo check -p rspaint-app --offline`. Se probaron regiones recortadas, bordes,
escrituras crudas, rellenos, copias, cambios de tamaño, clones divergentes y
restauración de todas las instantáneas de una secuencia de operaciones.

Las copias al iniciar trazos y previsualizaciones se abordaron en la sexta etapa.

## Cuarta etapa: historial por bloques

El editor usa ahora `History<HistorySnapshot>`. Cada instantánea guarda el árbol
de capas y sus propiedades, con píxeles divididos en bloques inmutables de
128 × 128 (hasta 64 KiB). Comparte con el nodo padre los bloques cuyos bytes
no cambiaron, incluyendo cambios directos mediante `data_mut`, sin depender
de los rectángulos sucios del editor. Si la identidad del buffer no cambió,
comparte directamente la tabla de bloques, útil para acciones de metadatos.

Las ramas siguen siendo independientes y conservan el mismo árbol de historial.
Restaurar reconstruye los buffers continuos que usa el dibujo y reutiliza los
buffers actuales si su identidad o contenido ya coincide. Se conservaron las
API `LayerStack` y `Document::restore` para instantáneas editables externas;
el historial interno usa las nuevas API `history_snapshot` y `restore_history`.
Las instantáneas no retienen referencias fuertes al buffer continuo de origen.

Se actualizó el benchmark existente para contar bloques únicos, sin materializar
imágenes completas para medirlos:

```sh
cargo run --release -p rspaint-editor --example history_memory_bench --offline
```

| Lienzo | Historial inicial | Después de 12 cambios de un píxel | Después de 12 renombrados adicionales | Con una rama adicional |
| --- | ---: | ---: | ---: | ---: |
| 2048 × 2048 | 16,000 MiB | 16,750 MiB | 16,750 MiB | 16,812 MiB |
| 6000 × 4000 | 91,553 MiB | 92,303 MiB | 92,303 MiB | 92,365 MiB |

Antes se midieron 208 MiB para el mismo caso de doce cambios en 2048 × 2048.
Ahora el crecimiento exacto es 786432 bytes, doce bloques de 64 KiB. El ejemplo
verifica ese total y que renombrar no agrega bloques. Estas cifras cuentan solo
RGBA retenido por el historial: excluyen tablas, metadatos, buffers de trabajo,
previsualizaciones, caché de composición y GPU. La primera instantánea ahora
almacena una copia por bloques además del buffer continuo vivo; el beneficio
crece al acumular ediciones. No hay compresión ni límite total del historial.

En la última ejecución, la mediana de guardar una instantánea modificada fue
4,139 ms en 2048 × 2048 y 26,321 ms en 6000 × 4000 (máximos 4,582 y 29,087 ms,
doce muestras). Esto todavía compara la capa modificada completa: se redujo la
memoria retenida, no el área recorrida al confirmar una edición. La primera
escritura compartida también sigue copiando el buffer continuo: medianas
3,275 y 31,146 ms. Transformaciones completas pueden añadir todos los bloques.

Verificación actual: 151 pruebas correctas entre core y editor;
`cargo check -p rspaint-app --offline`, formato y revisión de espacios correctos.
Las pruebas nuevas cubren bloques de borde, escrituras que cruzan bloques,
alfa, bytes sin cambios, restauración sin copias innecesarias, propiedades de
capas/grupos, ramas hermanas, saltos de historial, undo/redo y redimensionado.

El registro de bloques modificados se implementó en la quinta etapa.
Las previsualizaciones y el inicio del trazo aún usan buffers continuos con
copia por escritura.

## Tercera etapa: caché de composición

Se agregó una caché por documento, con bloques alineados de 64 × 64 y expulsión
del bloque usado menos recientemente. Retiene como máximo 32 MiB de RGBA y
2048 entradas; las firmas del árbol y los metadatos ocupan memoria adicional.
Reutiliza la pila estática situada debajo de la capa activa y los interiores
aislados de grupos estáticos, incluidos grupos superiores. Las capas superiores
sueltas siguen mezclándose contra el fondo actual: precomponerlas sobre
transparencia cambiaría el resultado de algunos modos de mezcla.

Las firmas comparan orden, tipo de nodo, visibilidad, opacidad, modo, dimensiones
e identidad débil del almacenamiento de píxeles. `Arc::make_mut` invalida esa
identidad al escribir, incluso sin actualizar las revisiones del editor.
La caché no conserva referencias fuertes a las imágenes fuente ni firmas de la
capa activa. Las pruebas comprueban que no agrega propietarios de sus píxeles
y que una mutación con un único propietario invalida la firma sin copiar el Vec.

```sh
cargo run --release -p rspaint-core --example composite_cache_bench --offline
```

Medición de CPU para ocho capas raster, seis de ellas estáticas semitransparentes,
una base blanca y la capa activa arriba. La variante agrupada pone esas seis
capas en un grupo con opacidad del 75 %. Se midieron 21 movimientos en regiones
de 64 × 48, cruzando límites de bloques y modificando la capa activa en cada
muestra. Cada resultado se compara con el compositor sin caché. Para la medición
fría se excluye la copia inicial del compuesto compartido; para las medianas se
precargan las regiones. Se usa el mismo núcleo de mezcla en ambos recorridos.

| Lienzo | Grupo estático | Sin caché mediana / p95 | Con caché mediana / p95 | Primera región sin caché precargada |
| --- | --- | --- | --- | --- |
| 1920 × 1080 | No | 0,537 / 0,561 ms | 0,083 / 0,101 ms | 2,992 ms |
| 1920 × 1080 | Sí | 0,548 / 0,559 ms | 0,080 / 0,086 ms | 2,704 ms |
| 6000 × 4000 | No | 0,532 / 0,573 ms | 0,075 / 0,099 ms | 2,531 ms |
| 6000 × 4000 | Sí | 0,543 / 0,565 ms | 0,079 / 0,084 ms | 2,652 ms |

La primera visita tiene costo adicional por poblar bloques completos. Se eligió
64 × 64 tras medir alrededor de 45 ms con bloques de 256 × 256 en la misma región
fría de 1920 × 1080 (aquella prueba también incluía la copia inicial del compuesto).
Las cifras no incluyen GPU ni latencia total de entrada. La caché beneficia la
reutilización local, no garantiza acelerar barridos completos que excedan sus
32 MiB ni cambios de propiedades que invaliden muchos bloques.

Verificación actual: 148 pruebas correctas entre core y editor y
`cargo check -p rspaint-app --offline`. Se verifican los 13 modos contra el
compositor de referencia, grupos, opacidad, visibilidad, cambios directos de
píxeles, orden, eliminación/restauración, cambio de capa activa, undo/redo,
ramas de historial, redimensionado, expulsión LRU y límite de memoria RGBA.

El historial por bloques se implementó en la cuarta etapa.

## Segunda etapa: composición y memoria del historial

La composición ahora procesa filas con un único acceso mutable al buffer y
copia directamente los píxeles opacos en modo Normal al 100 %. Se conservan
la fórmula y el redondeo del resto de los modos. Una comparación contra el
recorrido de referencia cubre colores y alfa variables, opacidades 0/1/50/99/100,
los 13 modos y regiones que exceden los límites de la fuente.

`Document::composite_rect` divide el trabajo en bloques de hasta 256 × 256.
Esto limita cada imagen temporal a 256 KiB, también para grupos anidados,
frente a 96 MB por imagen temporal de 6000 × 4000. El máximo simultáneo de
temporales sigue dependiendo de la profundidad de los grupos. No cambia la
memoria de las capas, el compuesto persistente ni las copias por escritura.
Esta división aún no es una caché: cada región solicitada se vuelve a componer.

Nueva ejecución del mismo benchmark de CPU, con siete muestras y calentamiento:

| Lienzo | Capas | Composición completa anterior | Actual | Región 256 × 256 anterior | Actual |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1920 × 1080 | 1 | 43,512 ms | 5,945 ms | 1,291 ms | 0,156 ms |
| 1920 × 1080 | 8 | 619,413 ms | 409,354 ms | 19,736 ms | 12,885 ms |
| 6000 × 4000 | 1 | 496,477 ms | 51,645 ms | 1,376 ms | 0,083 ms |
| 6000 × 4000 | 8 | 6631,761 ms | 4233,926 ms | 20,237 ms | 11,400 ms |

Las cifras anteriores provienen de la primera etapa; son ejecuciones separadas
en el mismo entorno, sujetas a carga y variación. Las ocho capas incluyen siete
capas semitransparentes completas. El costo de recomponer todo ese documento
sigue siendo alto: queda pendiente evitar ese trabajo mediante caché.

Para revisar el historial:

```sh
cargo run --release -p rspaint-editor --example history_memory_bench --offline
```

El ejemplo cuenta asignaciones RGBA únicas retenidas por los nodos de historial,
sin contar dos veces los buffers compartidos. En 2048 × 2048 midió 16 MiB
iniciales, 208 MiB después de doce cambios de un píxel y los mismos 208 MiB
después de doce renombrados. Una nueva rama elevó el total a 224 MiB y conservó
undo/redo. La mediana de la primera escritura fue 3,024 ms (máximo 4,118 ms).
No incluye metadatos, texturas GPU, buffers temporales ni el compuesto.
El historial por bloques continúa pendiente: esta etapa aporta la medición
reproducible y confirma que los metadatos ya comparten los píxeles correctamente.

Verificación actual: 145 pruebas correctas entre core y editor, incluyendo
equivalencia de composición por bloques con grupos anidados y recortes en bordes.

## Primera etapa

## Cambios implementados

- Las curvas restauran la union de los limites de la previsualizacion anterior
  y la nueva, ampliados por el grosor del pincel. La composicion y el upload
  siguen esa region. Cancelar restaura la imagen base e invalida esa region.
- Renombrar o activar una capa no invalida los pixeles. Los cambios de contenido
  pendientes, incluyendo selecciones o texto, conservan sus invalidaciones.
- Redimensionar deja la composicion al consumidor del upload pendiente, evitando
  calcularla dos veces en la misma actualizacion.
- Disolver usa coordenadas del lienzo incluso en regiones y grupos, para que la
  composicion parcial coincida con la completa.

## Medicion reproducible

```sh
cargo run --release -p rspaint-editor --example curve_layers_bench --offline
```

Resultados locales del 2026-10-30, en milisegundos. Mediana de siete muestras
despues de una muestra de calentamiento. CPU; no incluye transferencia GPU,
presentacion ni latencia completa de entrada. Una capa blanca de fondo y, para
el caso de ocho capas, siete capas completas semitransparentes en modo Normal.
Curva pequena con pincel de 3 px y controles entre (100,100) y (300,250).

| Lienzo | Capas | Composicion completa | Region 256x256 | Primera escritura compartida | Curva recorrido anterior* | Curva regional |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1920x1080 | 1 | 43.512 | 1.291 | 1.836 | 44.666 | 0.798 |
| 1920x1080 | 8 | 619.413 | 19.736 | 1.937 | 618.918 | 10.282 |
| 6000x4000 | 1 | 496.477 | 1.376 | 24.214 | 505.154 | 0.901 |
| 6000x4000 | 8 | 6631.761 | 20.237 | 23.439 | 7266.636 | 10.351 |

*El benchmark reconstruye el recorrido anterior: restauracion completa,
rasterizacion y composicion completa. No es una ejecucion de otro commit.
La curva regional mide movimientos posteriores al inicio; el costo de la
primera copia por escritura se mide por separado. Estas cifras dependen del
equipo y de la carga del sistema y no son garantias de FPS.

## Siguiente etapa pendiente

1. La cache por bloques y grupos se implementó en la tercera etapa. Queda por
   ampliar la medición a recorridos completos, muchas capas superiores sueltas
   y percentiles de latencia de la aplicación con GPU.
2. Las instantáneas por bloques se implementaron en la cuarta etapa. Quedan
   pendientes las copias temporales del buffer de trabajo y evitar el recorrido
   completo de la capa al detectar bloques modificados.
3. Medir memoria retenida y percentiles de latencia de la aplicacion real antes
   de decidir tamanos de bloque, limites de cache o presupuesto de historial.

No se cambio la representacion de pixeles ni del historial en esta etapa.

## Verificacion

`cargo test -p rspaint-core -p rspaint-editor --offline`: 143 pruebas correctas.
Incluye comparacion de curvas con rasterizacion directa, limpieza de previews,
cancelacion, bordes, seleccion, ausencia de uploads por metadatos y equivalencia
de composicion parcial/completa para los 13 modos dentro de grupos.
