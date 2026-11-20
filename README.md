# CuyLimeño

Asistente de investigación académica con **RAG (recuperación aumentada por generación)** sobre
**SoupDB**. Sube tus papers en PDF, pregunta en lenguaje natural y recibe respuestas con
**citas verificables**: documento, página y extracto, con el PDF original resaltado donde
aparece la cita.

---

## Resumen de la app

**Stack**

- **Frontend:** Svelte 5 + TypeScript + Tailwind (Vite).
- **Backend:** Rust con `axum`, que actúa de puente entre el navegador y SoupDB mediante el
  driver `rsoup` (`soupdb-driver`).
- **Base de datos:** SoupDB 0.8.0 (motor multimodal) con el plugin **INVERTED** para búsqueda
  de texto (BM25).
- **Generación:** GroqCloud (API compatible con OpenAI), con streaming SSE.

**Flujo**

1. Subes un PDF (máx. 50 MB).
2. El servidor extrae el texto **por páginas**, lo normaliza y lo divide en fragmentos
   (~300 palabras, máx. 500, con solape).
3. Los fragmentos se guardan en SoupDB y se indexan con un índice invertido (BM25).
4. Al preguntar, se recuperan los fragmentos más relevantes (con expansión de términos y un
   resumen de respaldo si no hay coincidencias).
5. El modelo responde en streaming y cada afirmación se marca con `[n]`; el panel derecho
   muestra la evidencia y al pulsarla se abre el **PDF original en la página citada**, con el
   fragmento resaltado.
6. Las conversaciones quedan en el historial lateral (renombrar, eliminar y carga paginada).

**Funciones principales**

- Ingesta de PDFs con estado `processing → ready` y visualización del documento original.
- Chat con streaming, Markdown, citas y panel de evidencias/fuentes.
- Selección de fuentes al instante (documentos concretos o toda la biblioteca).
- Historial en la barra lateral, con renombrado y paginación.
- Configuración de servicios y **cambio de modelo de Groq** persistido (sin reiniciar).
- Modo general: si las fuentes no cubren una pregunta general, responde con conocimiento
  general indicando que no está respaldado por los documentos.
- Robustez: reconexión del driver y reparación automática del índice si el motor pierde
  metadatos tras un reinicio no limpio.

---

## Demo rápida con Docker

### Ensayo y demo en vivo (Windows PowerShell)

| Repositorio | Papel en la demo |
|---|---|
| **CuyLimeno** | App web y API Rust. Recibe el PDF, extrae texto por página, crea fragmentos, consulta el motor y arma el contexto para GroqCloud. Presenta la respuesta y enlaza las citas con el PDF. |
| **SoupDB** | Motor que almacena los metadatos y fragmentos y recupera texto con el plugin `soupdb-inverted` y BM25. Compose usa la imagen publicada `0.8.0`. |
| **rsoup** | Driver TCP que usa el backend de CuyLimeno. Cargo lo incorpora como `soupdb-driver` 0.3.0 al compilar el servidor. |
| **SoupChef** | Cliente SQL opcional para mostrar las tablas e índices del mismo motor. Su versión web habla TCP desde el servidor Node. Su versión de escritorio usa rsoup con Tauri. |

El flujo es navegador → CuyLimeno → rsoup → SoupDB para recuperar fragmentos. CuyLimeno envía los fragmentos relevantes y la pregunta a GroqCloud para generar la respuesta. Los PDFs originales se guardan en el volumen de CuyLimeno. La recuperación documental usa texto y el índice invertido.

1. **Prepara el equipo antes de presentar.** Abre Docker Desktop con contenedores Linux. La primera compilación descarga imágenes y dependencias. GroqCloud requiere internet también durante la demo. Prepara un PDF con texto seleccionable y una pregunta cuya respuesta puedas ubicar en una página concreta. El límite predeterminado es de 50 MiB por PDF.

   ```powershell
   Set-Location 'C:\Users\yeimi\Downloads\BD2\Proyecto\CuyLimeno'
   docker version
   docker compose version
   if (-not (Test-Path .env)) { Copy-Item .env.example .env }
   notepad .env
   ```

   Completa `GROQ_API_KEY` en el archivo y guárdalo. Puedes conservar el valor de `GROQ_MODEL` de la plantilla. Usa un PDF de prueba que puedas compartir con GroqCloud.

2. **Construye y arranca.** Elige uno de los dos comandos. Para el segundo, `SoupChef` debe estar junto a `CuyLimeno` en la carpeta padre.

   ```powershell
   # Solo el RAG documental
   docker compose up -d --build --wait --wait-timeout 180

   # RAG más el inspector SQL opcional
   docker compose --profile inspeccion up -d --build --wait --wait-timeout 180
   ```

   Abre CuyLimeño en <http://localhost:8080>. Con el perfil activo, abre también SoupChef en <http://localhost:3000>. PostGIS pertenece a los experimentos de SoupDB. La demo usa un solo motor y un solo Compose.

3. **Comprueba el estado antes de subir el PDF.**

   ```powershell
   docker compose --profile inspeccion ps -a
   Invoke-RestMethod http://localhost:8080/api/health
   Invoke-RestMethod http://localhost:8080/api/capabilities
   ```

   `plugins` termina con código 0 tras instalar el índice. `engine` y `server` deben estar saludables y `web` en ejecución. En la API espera `database: connected`, `driver: rsoup` y `schema_configured`, `ingestion_available`, `retrieval_available` y `generation_available` en `true`. Este último indica que el servidor tiene configuración de generación. Envía una consulta de ensayo para confirmar que Groq acepta la clave y el modelo.

4. **Muestra la carga y una respuesta con cita.** En **Mis documentos → Añadir documentos → Seleccionar PDF**, carga el archivo y espera el estado **Listo**. Puedes usar `tests/fixtures/sample.pdf`, de tres páginas. En el chat abre **Fuentes**, selecciona ese PDF y pregunta «¿En qué página aparece el fragmento sobre el motor multimodal y qué dice?». El texto está en la página 3. Pulsa la cita en **Evidencias y citas** para mostrar el extracto y la página en el visor. Con otro PDF, usa una pregunta que incluya términos del documento para facilitar la recuperación BM25.

5. **Muestra el motor si activaste SoupChef.** Ejecuta la siguiente consulta para mostrar la tabla generada durante la ingesta. `page` vincula el fragmento con la página del PDF.

   ```sql
   SELECT document_id, page, pos, texto FROM cuy_chunks LIMIT 10;
   ```

   Para mostrar la recuperación léxica con el PDF de prueba, ejecuta la variante que usa el mismo índice que CuyLimeño.

   ```sql
   SELECT page, texto, SCORE()
   FROM cuy_chunks
   WHERE SEARCH(texto, 'motor multimodal') USING INVERTED
     WITH (k = 5, method = 'bm25')
   LIMIT 5;
   ```

6. **Reutiliza el ensayo el día de la demo.** Desde la misma carpeta ejecuta `docker compose up -d --wait --wait-timeout 180` con las imágenes ya construidas. Agrega `--profile inspeccion` antes de `up` si vas a mostrar SoupChef. Repite las comprobaciones del paso 3. Para detener la demo conservando los PDFs, los índices y las conversaciones, ejecuta `docker compose --profile inspeccion down`.

| Si ocurre esto | Cómo revisarlo |
|---|---|
| Puerto ocupado | Cambia `CUYLIMENO_WEB_PORT` o `SOUPCHEF_WEB_PORT` en `.env` y vuelve a ejecutar `up`. Usa el puerto nuevo en el navegador y en los chequeos HTTP. |
| Motor o índice pendiente | Ejecuta `docker compose logs --tail 100 plugins engine server` y revisa el estado antes de intentar cargar el archivo. |
| **Error** al procesar un PDF | Abre el PDF e intenta seleccionar texto. Para el ensayo usa el PDF de prueba o un documento con capa de texto. |
| Groq sin configurar o responde con error | Comprueba la clave, el modelo y la conexión a internet. Tras editar `.env`, ejecuta `docker compose up -d --wait` con el mismo perfil que usaste en el arranque. Si Groq sigue indisponible, muestra el PDF en **Mis documentos** y, con SoupChef, la consulta BM25 del paso 5. |

### Inicio rápido

Requisitos: Docker con Compose v2. La primera vez necesita internet (descarga de imágenes e
instalación del plugin de texto).

```bash
# 1) Configura la clave (nunca se versiona ni se incluye en las imágenes)
cp .env.example .env
# edita .env y completa GROQ_API_KEY (opcional: GROQ_MODEL)

# 2) Levanta la demo
make demo
# equivalente: docker compose up -d --build

# 3) Abre la app
# http://localhost:8080
```

Servicios que levanta el compose:

| Servicio | Descripción |
|---|---|
| `plugins` | Instala `soupdb-inverted` en un volumen compartido. |
| `engine` | SoupDB `0.8.0`, con datos persistidos en un volumen. |
| `server` | Puente Rust (`axum` + `rsoup`) en la red interna. |
| `web` | Nginx sirviendo el frontend y proxificando `/api` (SSE incluido). |

Comandos útiles:

```bash
docker compose ps
docker compose logs -f server
make demo-down            # detiene la demo (los volúmenes se conservan)
docker compose down -v    # borra también los datos
```

> La imagen oficial del gestor y las imágenes de la app son **multi-arquitectura**
> (`linux/amd64` y `linux/arm64`). En un x86-64 el compose compila de forma nativa.

### Variables de entorno

La demo lee `.env` automáticamente para inyectar variables en el contenedor `server`.
`.env` está ignorado por Git y por Docker; la clave nunca se guarda en las imágenes.

| Variable | Default | Uso |
|---|---|---|
| `GROQ_API_KEY` | — | Obligatoria para generar respuestas. |
| `GROQ_MODEL` | `openai/gpt-oss-20b` | Modelo de chat (se puede cambiar también desde la UI). |
| `GROQ_API_BASE_URL` | `https://api.groq.com/openai/v1` | Proxy compatible si aplica. |
| `CUYLIMENO_WEB_PORT` | `8080` | Puerto del frontend de la demo. |
| `SOUPDB_HOST` / `SOUPDB_PORT` | `engine` / `55432` | Conexión al gestor (en compose). |
| `SOUPDB_TIMEOUT_MS` | `10000` | Timeout de operaciones contra SoupDB. |
| `CUYLIMENO_DATA_DIR` | `/app/data/cuylimeno` (compose) | Carpeta de PDFs subidos. |

Si no hay `GROQ_API_KEY`, la app funciona para ingesta y consulta, pero la generación queda
deshabilitada hasta configurarla.

---

## Desarrollo local (sin Docker)

Requisitos: Node 22+, Rust (edición 2021) y un SoupDB accesible en `127.0.0.1:55432` con el
plugin `inverted` activado.

```bash
cp .env.example .env          # configura GROQ_API_KEY
npm install
npm run server                # backend en :3001 (carga .env automáticamente)
npm run dev                   # frontend en :5173 (proxy /api → :3001)
```

El backend usa el crate publicado **`soupdb-driver = "0.3.0"`**, por lo que no necesita el
repositorio de `rsoup` al lado.

### Pruebas

```bash
npm test                 # Vitest (cliente HTTP y SSE)
npm run check            # svelte-check
npx playwright test      # E2E (desktop y móvil)
cd server && cargo test  # unitarios del backend
# Integración con un motor real:
cd server && SOUPDB_TEST=1 cargo test -- --ignored
```

---

## Arquitectura multi-arquitectura

Las imágenes propias se publican con buildx para `linux/amd64` y `linux/arm64`:

```bash
docker login ghcr.io
make images              # docker buildx bake --push
```

- `ghcr.io/bd2-project/cuylimeno-web:latest`
- `ghcr.io/bd2-project/cuylimeno-server:latest`
- `ghcr.io/bd2-project/soupdb:0.8.0` (imagen oficial del gestor, multi-arch)

Para verificar el build x86-64 en un Mac ARM (QEMU):

```bash
make images-amd64
```

---

## Estructura

```
CuyLimeno/
├── src/                  Frontend Svelte (vistas, chat, visor PDF, Markdown)
├── server/               Backend Rust (axum + rsoup + Groq)
│   └── src/
│       ├── db.rs         Pool, bootstrap y reparación del índice
│       ├── ingest.rs     Extracción de PDF y chunking
│       ├── retrieval.rs  BM25, expansión y fallback de resumen
│       ├── llm.rs        Cliente Groq con streaming SSE
│       └── api/          Endpoints HTTP
├── tests/                Playwright + PDF de fixture
├── docker/               Configuración de nginx
├── compose.yaml          Demo completa (gestor + backend + frontend)
├── Dockerfile            Imagen del frontend
└── Makefile              Atajos: demo, images, images-amd64
```

---

## Limitaciones conocidas

- Solo PDF **con capa de texto**; los escaneados necesitan OCR (no incluido).
- La recuperación es léxica (BM25) más un fallback de resumen; no usa embeddings vectoriales.
- La app es de un solo espacio local, sin autenticación.
- La clave de Groq se inyecta por entorno; para un despliegue real conviene un gestor de
  secretos.

## Licencia

MIT. Consulta `LICENSE`.
