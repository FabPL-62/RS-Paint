# Rendimiento

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
