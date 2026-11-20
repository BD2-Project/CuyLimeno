use std::{collections::HashSet, sync::Mutex, time::Duration};

use rsoup::{Connection, DriverError, QueryResult, ResultSet, Value};
use tokio::{
    sync::{Semaphore, SemaphorePermit},
    time::timeout,
};

use crate::{config::Config, error::ApiError};

/// Tiempo máximo para comprobar que una conexión ociosa sigue viva antes de
/// reutilizarla. El motor cierra conexiones inactivas (`SOCKET_TIMEOUT`), así
/// que nunca se debe asumir que una conexión del pool sigue abierta.
const IDLE_PING_TIMEOUT: Duration = Duration::from_millis(1500);

#[derive(Debug)]
pub enum DbError {
    Unavailable(String),
    Timeout,
    Driver { code: u8, message: String },
}

impl DbError {
    pub fn message(&self) -> &str {
        match self {
            DbError::Unavailable(message) => message,
            DbError::Timeout => "timeout",
            DbError::Driver { message, .. } => message,
        }
    }

    pub fn is_already_exists(&self) -> bool {
        self.message().contains("already exists")
    }

    pub fn is_blocked(&self) -> bool {
        self.message().contains("blocked")
    }

    /// ¿El fallo es de transporte (conexión cerrada/reiniciada) y no de SQL?
    /// En ese caso la conexión no sirve y debe descartarse del pool.
    pub fn is_transport(&self) -> bool {
        match self {
            DbError::Unavailable(_) => true,
            DbError::Timeout => false,
            DbError::Driver { message, .. } => {
                let message = message.to_ascii_lowercase();
                message.contains("eof")
                    || message.contains("broken pipe")
                    || message.contains("reset")
                    || message.contains("closed")
                    || message.contains("connection")
                    || message.contains("conexión")
            }
        }
    }
}

impl From<DriverError> for DbError {
    fn from(error: DriverError) -> Self {
        DbError::Driver {
            code: error.code,
            message: error.message,
        }
    }
}

impl std::fmt::Display for DbError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DbError::Unavailable(message) => write!(formatter, "motor no disponible: {message}"),
            DbError::Timeout => write!(formatter, "el motor no respondió a tiempo"),
            DbError::Driver { code, message } => write!(formatter, "error {code}: {message}"),
        }
    }
}

impl From<DbError> for ApiError {
    fn from(error: DbError) -> Self {
        match error {
            DbError::Unavailable(message) => ApiError::service_unavailable(
                "DATABASE_UNAVAILABLE",
                format!("No se pudo contactar con SoupDB mediante rsoup: {message}"),
            ),
            DbError::Timeout => ApiError::service_unavailable(
                "DATABASE_TIMEOUT",
                "SoupDB no respondió dentro del tiempo configurado.",
            ),
            DbError::Driver { message, .. } => ApiError::internal(
                "DATABASE_ERROR",
                format!("SoupDB rechazó la operación: {message}"),
            ),
        }
    }
}

/// Pool mínimo de conexiones rsoup: reutiliza conexiones ociosas y limita la
/// concurrencia con un semáforo. Las conexiones con errores de transporte se
/// descartan en lugar de reutilizarse.
pub struct Db {
    host: String,
    port: u16,
    timeout: Duration,
    permits: Semaphore,
    max_idle: usize,
    idle: Mutex<Vec<Connection>>,
}

impl Db {
    pub fn new(config: &Config) -> Self {
        Self {
            host: config.database_host.clone(),
            port: config.database_port,
            timeout: config.database_timeout,
            permits: Semaphore::new(config.pool_size),
            max_idle: config.pool_size,
            idle: Mutex::new(Vec::new()),
        }
    }

    pub async fn acquire(&self) -> Result<Pooled<'_>, DbError> {
        let permit = self
            .permits
            .acquire()
            .await
            .map_err(|_| DbError::Unavailable("pool de conexiones cerrado".to_owned()))?;
        let idle = self
            .idle
            .lock()
            .map_err(|_| DbError::Unavailable("pool de conexiones envenenado".to_owned()))?
            .pop();
        let connection = match idle {
            // El motor cierra conexiones inactivas: comprobar antes de reutilizar.
            Some(mut connection) => match timeout(IDLE_PING_TIMEOUT, connection.ping()).await {
                Ok(Ok(())) => connection,
                _ => self.connect().await?,
            },
            None => self.connect().await?,
        };
        Ok(Pooled {
            db: self,
            connection: Some(connection),
            _permit: permit,
        })
    }

    async fn connect(&self) -> Result<Connection, DbError> {
        timeout(self.timeout, Connection::connect(&self.host, self.port))
            .await
            .map_err(|_| DbError::Timeout)?
            .map_err(|error| DbError::Unavailable(format!("{error}")))
    }

    pub async fn ping(&self) -> Result<(), DbError> {
        let mut connection = self.acquire().await?;
        connection.ping().await
    }

    /// Lee un valor de la tabla de metadatos de CuyLimeno.
    pub async fn meta_value(&self, key: &str) -> Result<Option<String>, DbError> {
        let mut connection = self.acquire().await?;
        let result = connection
            .result(&format!(
                "SELECT value FROM cuy_meta WHERE key_name = {} LIMIT 1",
                sql_literal(key)
            ))
            .await?;
        Ok(result
            .rows
            .first()
            .and_then(|row| row.first())
            .and_then(as_text))
    }

    /// Guarda un valor de metadatos (el motor no ejecuta UPDATE).
    pub async fn set_meta(&self, key: &str, value: &str) -> Result<(), DbError> {
        let mut connection = self.acquire().await?;
        connection
            .transaction(&[
                format!(
                    "DELETE FROM cuy_meta WHERE key_name = {}",
                    sql_literal(key)
                ),
                format!(
                    "INSERT INTO cuy_meta VALUES ({}, {})",
                    sql_literal(key),
                    sql_literal(value)
                ),
            ])
            .await
    }

    /// Prepara el esquema documental de CuyLimeno (idempotente).
    ///
    /// Devuelve si el índice INVERTED quedó operativo (probado con un SEARCH real).
    pub async fn bootstrap(&self, config: &Config) -> Result<bool, DbError> {
        let mut connection = self.acquire().await?;
        ensure_plugin(&mut connection, &config.inverted_storage_root).await?;
        ensure_tables(&mut connection).await?;
        match ensure_index(&mut connection).await {
            Ok(()) => Ok(true),
            Err(error) => {
                eprintln!("CuyLimeno: no se pudo reparar el índice INVERTED: {error}");
                Ok(false)
            }
        }
    }
}

pub struct Pooled<'a> {
    db: &'a Db,
    connection: Option<Connection>,
    _permit: SemaphorePermit<'a>,
}

impl Pooled<'_> {
    pub async fn query(&mut self, sql: &str) -> Result<QueryResult, DbError> {
        let connection = self
            .connection
            .as_mut()
            .ok_or_else(|| DbError::Unavailable("conexión descartada".to_owned()))?;
        match timeout(self.db.timeout, connection.query(sql)).await {
            Err(_) => {
                self.connection = None;
                Err(DbError::Timeout)
            }
            Ok(Err(error)) => {
                let error = DbError::from(error);
                if error.is_transport() {
                    self.connection = None;
                }
                Err(error)
            }
            Ok(Ok(result)) => Ok(result),
        }
    }

    pub async fn result(&mut self, sql: &str) -> Result<ResultSet, DbError> {
        match self.query(sql).await? {
            QueryResult::ResultSet(result) => Ok(result),
            QueryResult::Affected(_) => Ok(ResultSet {
                columns: Vec::new(),
                rows: Vec::new(),
            }),
        }
    }

    pub async fn execute(&mut self, sql: &str) -> Result<u32, DbError> {
        match self.query(sql).await? {
            QueryResult::Affected(affected) => Ok(affected),
            QueryResult::ResultSet(_) => Ok(0),
        }
    }

    pub async fn ping(&mut self) -> Result<(), DbError> {
        let connection = self
            .connection
            .as_mut()
            .ok_or_else(|| DbError::Unavailable("conexión descartada".to_owned()))?;
        match timeout(self.db.timeout, connection.ping()).await {
            Err(_) => {
                self.connection = None;
                Err(DbError::Timeout)
            }
            Ok(Err(error)) => {
                self.connection = None;
                Err(DbError::Unavailable(format!("{error}")))
            }
            Ok(Ok(())) => Ok(()),
        }
    }

    /// Ejecuta varias sentencias dentro de una transacción.
    pub async fn transaction(&mut self, statements: &[String]) -> Result<(), DbError> {
        if statements.is_empty() {
            return Ok(());
        }
        self.begin().await?;
        for statement in statements {
            if let Err(error) = self.query(statement).await {
                let _ = self.rollback().await;
                return Err(error);
            }
        }
        self.commit().await?;
        Ok(())
    }

    async fn begin(&mut self) -> Result<(), DbError> {
        let connection = self
            .connection
            .as_mut()
            .ok_or_else(|| DbError::Unavailable("conexión descartada".to_owned()))?;
        match timeout(self.db.timeout, connection.begin()).await {
            Err(_) => {
                self.connection = None;
                Err(DbError::Timeout)
            }
            Ok(Err(error)) => {
                let error = DbError::from(error);
                if error.is_transport() {
                    self.connection = None;
                }
                Err(error)
            }
            Ok(Ok(())) => Ok(()),
        }
    }

    async fn commit(&mut self) -> Result<(), DbError> {
        let connection = self
            .connection
            .as_mut()
            .ok_or_else(|| DbError::Unavailable("conexión descartada".to_owned()))?;
        match timeout(self.db.timeout, connection.commit()).await {
            Err(_) => {
                self.connection = None;
                Err(DbError::Timeout)
            }
            Ok(Err(error)) => {
                let error = DbError::from(error);
                if error.is_transport() {
                    self.connection = None;
                }
                Err(error)
            }
            Ok(Ok(())) => Ok(()),
        }
    }

    async fn rollback(&mut self) -> Result<(), DbError> {
        let connection = self
            .connection
            .as_mut()
            .ok_or_else(|| DbError::Unavailable("conexión descartada".to_owned()))?;
        match timeout(self.db.timeout, connection.rollback()).await {
            Err(_) => {
                self.connection = None;
                Err(DbError::Timeout)
            }
            Ok(Err(error)) => {
                let error = DbError::from(error);
                if error.is_transport() {
                    self.connection = None;
                }
                Err(error)
            }
            Ok(Ok(())) => Ok(()),
        }
    }
}

impl Drop for Pooled<'_> {
    fn drop(&mut self) {
        if let Some(connection) = self.connection.take() {
            if let Ok(mut idle) = self.db.idle.lock() {
                if idle.len() < self.db.max_idle {
                    idle.push(connection);
                }
            }
        }
    }
}

async fn ensure_plugin(connection: &mut Pooled<'_>, storage_root: &str) -> Result<(), DbError> {
    let plugins = connection.result("SELECT plugin_name FROM SysPlugins").await?;
    let active: HashSet<String> = plugins
        .rows
        .iter()
        .filter_map(|row| row.first().and_then(as_text))
        .collect();
    if active.contains("INVERTED") {
        return Ok(());
    }
    connection
        .execute(&format!(
            "CREATE EXTENSION inverted WITH (storage_root = {})",
            sql_literal(storage_root)
        ))
        .await?;
    Ok(())
}

const TABLES: &[(&str, &str)] = &[
    (
        "cuy_documents",
        "CREATE TABLE cuy_documents (id VARCHAR(36), title TEXT, authors TEXT, year INT, \
         filename TEXT, page_count INT, status VARCHAR(16), created_at TEXT)",
    ),
    (
        "cuy_chunks",
        "CREATE TABLE cuy_chunks (document_id VARCHAR(36), pos INT, page INT, texto TEXT)",
    ),
    (
        "cuy_collections",
        "CREATE TABLE cuy_collections (id VARCHAR(36), name TEXT, description TEXT, created_at TEXT)",
    ),
    (
        "cuy_collection_documents",
        "CREATE TABLE cuy_collection_documents (collection_id VARCHAR(36), document_id VARCHAR(36))",
    ),
    (
        "cuy_conversations",
        "CREATE TABLE cuy_conversations (id VARCHAR(36), title TEXT, document_ids TEXT, \
         collection_ids TEXT, updated_at TEXT)",
    ),
    (
        "cuy_messages",
        "CREATE TABLE cuy_messages (id VARCHAR(36), conversation_id VARCHAR(36), \
         role VARCHAR(16), content TEXT, created_at TEXT)",
    ),
    (
        "cuy_citations",
        "CREATE TABLE cuy_citations (message_id VARCHAR(36), document_id VARCHAR(36), \
         title TEXT, page INT, excerpt TEXT)",
    ),
    (
        "cuy_meta",
        "CREATE TABLE cuy_meta (key_name VARCHAR(64), value TEXT)",
    ),
];

async fn ensure_tables(connection: &mut Pooled<'_>) -> Result<(), DbError> {
    let tables = connection.result("SELECT table_name FROM SysTables").await?;
    let existing: HashSet<String> = tables
        .rows
        .iter()
        .filter_map(|row| row.first().and_then(as_text))
        .collect();
    for (name, ddl) in TABLES {
        if !existing.contains(*name) {
            connection.execute(ddl).await?;
        }
    }
    Ok(())
}

pub const INDEX_DDL: &str = "CREATE INDEX idx_cuy_chunks ON cuy_chunks (texto) USING INVERTED \
                             WITH (method = 'bm25')";
const INDEX_PROBE: &str = "SELECT document_id FROM cuy_chunks \
                           WHERE SEARCH(texto, 'cuylimeno_probe') USING INVERTED \
                           WITH (k = 1, method = 'bm25') LIMIT 1";

/// Comprueba que el binding del índice está vivo ejecutando un SEARCH real.
/// El motor puede perder el metadato (`SysPluginIndexes`) al reiniciarse sin
/// apagado limpio aunque los archivos físicos sobrevivan; por eso no se puede
/// asumir que "el índice fue creado" significa que sigue operativo.
pub(crate) async fn probe_index(connection: &mut Pooled<'_>) -> Result<(), DbError> {
    connection.result(INDEX_PROBE).await?;
    Ok(())
}

/// Garantiza un índice INVERTED operativo, reparándolo si el motor perdió el
/// binding (crea una generación nueva si quedaron archivos físicos huérfanos).
pub(crate) async fn ensure_index(connection: &mut Pooled<'_>) -> Result<(), DbError> {
    if probe_index(connection).await.is_ok() {
        return Ok(());
    }
    match connection.execute(INDEX_DDL).await {
        Ok(_) => {}
        Err(error) if error.is_already_exists() || error.message().contains("physical") => {
            let generation = format!("idx_cuy_chunks_r{}", chrono::Utc::now().timestamp());
            connection
                .execute(&format!(
                    "CREATE INDEX {generation} ON cuy_chunks (texto) USING INVERTED \
                     WITH (method = 'bm25')"
                ))
                .await?;
            if let Err(persist_error) =
                persist_meta(connection, "chunks_index_name", &generation).await
            {
                eprintln!("CuyLimeno: no se pudo registrar el índice {generation}: {persist_error}");
            }
        }
        Err(error) => return Err(error),
    }
    probe_index(connection).await
}

async fn persist_meta(
    connection: &mut Pooled<'_>,
    key: &str,
    value: &str,
) -> Result<(), DbError> {
    connection
        .transaction(&[
            format!(
                "DELETE FROM cuy_meta WHERE key_name = {}",
                sql_literal(key)
            ),
            format!(
                "INSERT INTO cuy_meta VALUES ({}, {})",
                sql_literal(key),
                sql_literal(value)
            ),
        ])
        .await
}

/// Escapa un literal de texto para SQL: duplica comillas simples y elimina
/// bytes nulos (el motor no tiene parametrización para todos los contextos,
/// p. ej. `SEARCH(col, 'texto')` exige un literal).
pub fn sql_escape(value: &str) -> String {
    value.replace('\0', "").replace('\'', "''")
}

pub fn sql_literal(value: &str) -> String {
    format!("'{}'", sql_escape(value))
}

pub fn as_text(value: &Value) -> Option<String> {
    match value {
        Value::Text(text) => Some(text.clone()),
        Value::Int(number) => Some(number.to_string()),
        _ => None,
    }
}

pub fn as_int(value: &Value) -> Option<i32> {
    match value {
        Value::Int(number) => Some(*number),
        Value::Float(number) => Some(*number as i32),
        Value::Text(text) => text.parse().ok(),
        _ => None,
    }
}

pub fn as_float(value: &Value) -> Option<f64> {
    match value {
        Value::Float(number) => Some(*number),
        Value::Int(number) => Some(*number as f64),
        Value::Text(text) => text.parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapa_comillas_y_elimina_nulos() {
        assert_eq!(sql_escape("O'Higgins"), "O''Higgins");
        assert_eq!(sql_escape("a\0b"), "ab");
        assert_eq!(sql_literal("d'Artagnan"), "'d''Artagnan'");
    }

    #[test]
    fn reconoce_errores_del_motor() {
        let exists = DbError::Driver {
            code: 1,
            message: "index 'idx_cuy_chunks' already exists".to_owned(),
        };
        assert!(exists.is_already_exists());
        let blocked = DbError::Driver {
            code: 1,
            message: "ranked index maintenance is blocked".to_owned(),
        };
        assert!(blocked.is_blocked());
    }

    #[test]
    fn distingue_fallos_de_transporte_de_errores_sql() {
        for message in [
            "early eof",
            "Broken pipe (os error 32)",
            "Connection reset by peer (os error 54)",
            "conexión cerrada por el gestor",
        ] {
            assert!(
                DbError::Driver {
                    code: 1,
                    message: message.to_owned(),
                }
                .is_transport(),
                "debería ser transporte: {message}"
            );
        }
        assert!(!DbError::Driver {
            code: 2,
            message: "table 'cuy_documents' already exists".to_owned(),
        }
        .is_transport());
        assert!(!DbError::Driver {
            code: 3,
            message: "unsupported statement UpdateStatement".to_owned(),
        }
        .is_transport());
        assert!(DbError::Unavailable("conexión rechazada".to_owned()).is_transport());
        assert!(!DbError::Timeout.is_transport());
    }

    /// Integración opcional contra el motor real:
    /// `SOUPDB_TEST=1 cargo test -- --ignored`.
    #[tokio::test]
    #[ignore = "requiere SOUPDB_TEST=1 y el motor SoupDB corriendo"]
    async fn bootstrap_idempotente_contra_el_motor() {
        if std::env::var("SOUPDB_TEST").ok().as_deref() != Some("1") {
            return;
        }
        let config = Config::from_env().expect("configuración válida");
        let db = Db::new(&config);
        db.bootstrap(&config).await.expect("primer bootstrap");
        db.bootstrap(&config).await.expect("segundo bootstrap idempotente");
    }

    /// Verificación V1 del plan: SEARCH BM25 devuelve scores y el índice admite
    /// mantenimiento transaccional (INSERT/DELETE dentro de BEGIN/COMMIT).
    #[tokio::test]
    #[ignore = "requiere SOUPDB_TEST=1 y el motor SoupDB corriendo"]
    async fn busqueda_bm25_y_mantenimiento_transaccional() {
        if std::env::var("SOUPDB_TEST").ok().as_deref() != Some("1") {
            return;
        }
        let config = Config::from_env().expect("configuración válida");
        let db = Db::new(&config);
        db.bootstrap(&config).await.expect("bootstrap");
        let id = uuid::Uuid::new_v4().to_string();
        let mut connection = db.acquire().await.expect("conexión");
        connection
            .execute(&format!(
                "INSERT INTO cuy_documents VALUES ({}, 'Prueba', '[]', NULL, 'prueba.pdf', 1, 'ready', '2026-01-01T00:00:00Z')",
                sql_literal(&id)
            ))
            .await
            .expect("documento de prueba");

        let chunks = vec![
            format!(
                "INSERT INTO cuy_chunks VALUES ({}, 1, 1, 'bases de datos espaciales y recuperacion de informacion')",
                sql_literal(&id)
            ),
            format!(
                "INSERT INTO cuy_chunks VALUES ({}, 2, 1, 'indice invertido y busqueda de texto con bm25')",
                sql_literal(&id)
            ),
        ];
        // Regla del plan: DML sobre tabla indexada dentro de transacción.
        connection
            .transaction(&chunks)
            .await
            .expect("INSERT transaccional sobre el índice INVERTED");

        let result = connection
            .result(&format!(
                "SELECT document_id, page, texto, SCORE() AS score FROM cuy_chunks \
                 WHERE SEARCH(texto, 'invertido') USING INVERTED WITH (k = 10, method = 'bm25') LIMIT 10"
            ))
            .await
            .expect("búsqueda BM25");
        assert!(!result.rows.is_empty(), "SEARCH debe devolver fragmentos");
        assert!(
            result
                .rows
                .iter()
                .any(|row| row.first().and_then(as_text).as_deref() == Some(id.as_str())),
            "el fragmento insertado debe aparecer"
        );
        assert!(
            result
                .rows
                .iter()
                .any(|row| row.get(3).and_then(as_float).is_some()),
            "SCORE() debe devolver un FLOAT"
        );

        connection
            .transaction(&[format!(
                "DELETE FROM cuy_chunks WHERE document_id = {}",
                sql_literal(&id)
            )])
            .await
            .expect("DELETE transaccional sobre el índice INVERTED");
        connection
            .execute(&format!(
                "DELETE FROM cuy_documents WHERE id = {}",
                sql_literal(&id)
            ))
            .await
            .expect("limpieza del documento de prueba");
    }
}
