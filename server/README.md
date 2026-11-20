# Puente HTTP de CuyLimeno

Servidor Rust/Axum para el frontend web. Utiliza el driver local `../../rsoup`
(`soupdb-driver`, librería `rsoup`) para hablar con SoupDB mediante TCP.
Prepara el esquema documental (`cuy_documents`, `cuy_chunks` y sus índices) al arrancar,
fusiona el índice invertido instalado por el plugin `soupdb-inverted` y no expone SQL
arbitrario al navegador.

Requiere Rust y el checkout hermano `rsoup`. Desde la raíz:

```powershell
cargo run --manifest-path server/Cargo.toml
```

El frontend debe utilizar el proxy `/api` de desarrollo o un reverse proxy en producción.
El servidor no habilita CORS abierto. La configuración se lee del entorno del proceso y,
para desarrollo, también de `.env`, `../.env` o `server/.env` mediante `dotenvy`; los
valores ya exportados al proceso tienen prioridad. El límite de subida es de 50 MiB por
archivo y se ajusta con `CUYLIMENO_MAX_UPLOAD_MB`.

| Variable | Valor predeterminado | Uso |
| --- | --- | --- |
| `CUYLIMENO_SERVER_ADDR` | `127.0.0.1:3001` | Dirección del servidor HTTP |
| `SOUPDB_HOST` | `127.0.0.1` | Host TCP del gestor |
| `SOUPDB_PORT` | `55432` | Puerto TCP del gestor |
| `SOUPDB_TIMEOUT_MS` | `10000` | Límite de conexión y ping (1-120000 ms) |
| `SOUPDB_POOL_SIZE` | `4` | Conexiones TCP simultáneas (1-32) |
| `CUYLIMENO_DATA_DIR` | `./data/cuylimeno` | PDFs y datos persistentes |
| `CUYLIMENO_MAX_UPLOAD_MB` | `50` | Tamaño máximo de PDF por subida (1-500) |
| `SOUPDB_INVERTED_STORAGE_ROOT` | `/app/data/plugins/inverted` | Almacenamiento del índice invertido |
| `GROQ_API_KEY` | sin configurar | Clave reservada al servidor |
| `GROQ_MODEL` | sin configurar | Identificador de modelo elegido por el usuario |
| `GROQ_API_BASE_URL` | `https://api.groq.com/openai/v1` | Base de la API de generación |
| `GROQ_TIMEOUT_MS` | `120000` | Límite de la petición de generación (1000-600000 ms) |

GroqCloud solo se llama al enviar una consulta de chat con fuentes documentales. La clave
nunca se devuelve por HTTP. `groq_configured` indica solamente que clave y modelo están
presentes: no verifica credenciales ni disponibilidad del proveedor.
`generation_available` exige, además, que el esquema documental esté listo.

| Método | Ruta | Resultado |
| --- | --- | --- |
| GET | `/api/health` | 200 tras conexión y PING/PONG real; 503 ante fallo o timeout |
| GET | `/api/capabilities` | Estado del esquema, la ingestión, la recuperación y la generación |
| GET, POST | `/api/documents` | Listado de documentos; POST sube un PDF (`multipart/form-data`) |
| GET, DELETE | `/api/documents/{id}` | Detalle y borrado de un documento |
| GET | `/api/documents/{id}/content` | Fragmentos extraídos del documento |
| GET | `/api/documents/{id}/file` | Descarga del PDF original |
| GET, POST | `/api/collections` | Listado y creación de colecciones |
| GET, PATCH, DELETE | `/api/collections/{id}` | Detalle, renombrado y borrado de una colección |
| POST | `/api/collections/{id}/documents` | Añadir documentos a una colección |
| DELETE | `/api/collections/{id}/documents/{document_id}` | Quitar un documento de una colección |
| GET, POST | `/api/conversations` | Listado paginado y creación de conversaciones |
| GET, PATCH, DELETE | `/api/conversations/{id}` | Detalle, renombrado y borrado de una conversación |
| POST | `/api/chat` | Recuperación por BM25, respuesta generada y citas (streaming) |
| GET | `/api/models` | Modelos de generación disponibles |
| PUT | `/api/settings/model` | Cambia el modelo activo |

Si el esquema documental aún no está listo, las rutas que lo necesitan devuelven
`DOCUMENT_SCHEMA_NOT_CONFIGURED` en lugar de listas vacías. `src/contracts.rs` recoge
los DTOs HTTP del contrato. La plataforma es local y personal, sin autenticación por
decisión de alcance.

Los errores usan `{ "error": { "code": "...", "message": "..." } }`.
Las rutas desconocidas devuelven 404 y los métodos no admitidos 405 con ese formato.

Comprobaciones:

```powershell
cargo fmt --manifest-path server/Cargo.toml --check
cargo test --manifest-path server/Cargo.toml
cargo clippy --manifest-path server/Cargo.toml --all-targets -- -D warnings
```
