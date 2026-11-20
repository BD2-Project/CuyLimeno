use std::collections::{BTreeMap, HashMap, HashSet};

use crate::{
    contracts::Citation,
    db::{as_float, as_int, as_text, ensure_index, sql_literal, Db, DbError, Pooled},
    ingest::recortar,
};

pub const TOP_K: usize = 30;
pub const MAX_FRAGMENTOS: usize = 8;
pub const EXCERPT_WORDS: usize = 240;

const MIN_HITS: usize = 3;
const MAX_TERMS: usize = 8;
const OVERVIEW_DOCS: usize = 5;
const OVERVIEW_CHUNKS_PER_DOC: usize = 2;
const OVERVIEW_SCAN_LIMIT: usize = 500;

const STOPWORDS: &[&str] = &[
    // Español
    "de", "la", "el", "los", "las", "un", "una", "unos", "unas", "que", "y", "o", "en",
    "con", "por", "para", "del", "al", "se", "su", "sus", "es", "son", "esta", "está",
    "estan", "están", "como", "más", "mas", "pero", "si", "no", "lo", "le", "les", "me",
    "te", "nos", "os", "mi", "tu", "mis", "tus", "hay", "sobre", "entre", "cuando",
    "donde", "dónde", "cual", "cuál", "cuales", "cuáles", "quien", "quién", "cómo",
    "cuanto", "cuánto", "cuantos", "cuántos", "ser", "estar", "fue", "han", "ha", "he",
    "hemos", "tiene", "tienen", "tienes", "hace", "hacen", "muy", "también", "tambien",
    "solo", "sólo", "ya", "yo", "él", "ella", "ellos", "ellas", "nosotros", "vosotros",
    "usted", "ustedes", "mismo", "misma", "otro", "otra", "cada", "todo", "toda",
    // Inglés
    "the", "and", "or", "of", "to", "in", "on", "for", "with", "is", "are", "was",
    "were", "be", "been", "this", "that", "these", "those", "it", "its", "as", "at",
    "by", "from", "what", "which", "who", "how", "why", "do", "does", "did", "a", "an",
    "i", "you", "we", "they", "about", "into", "than",
];

#[derive(Debug, Clone)]
pub struct FragmentoRecuperado {
    pub document_id: String,
    pub pos: i32,
    pub page: u32,
    pub texto: String,
    pub score: f64,
}

/// Términos significativos de la consulta (sin stopwords ni duplicados).
pub fn significant_terms(query: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut terms = Vec::new();
    for raw in query.to_lowercase().split(|character: char| !character.is_alphanumeric()) {
        let term = raw.trim();
        if term.chars().count() < 3 || STOPWORDS.contains(&term) {
            continue;
        }
        if !seen.insert(term.to_owned()) {
            continue;
        }
        terms.push(term.to_owned());
        if terms.len() >= MAX_TERMS {
            break;
        }
    }
    terms
}

/// Recupera fragmentos: BM25 con la frase completa; si hay pocos aciertos,
/// busca por términos significativos y fusiona; si aun así no hay nada,
/// devuelve los primeros fragmentos de los documentos como contexto de resumen.
pub async fn recuperar(
    db: &Db,
    consulta: &str,
    alcance: &Option<HashSet<String>>,
) -> Result<Vec<FragmentoRecuperado>, DbError> {
    if consulta.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut connection = db.acquire().await?;
    let mut fragmentos = search(&mut connection, consulta, TOP_K).await?;
    if fragmentos.len() < MIN_HITS {
        for term in significant_terms(consulta) {
            let hits = search(&mut connection, &term, TOP_K / 2).await?;
            merge(&mut fragmentos, hits);
            if fragmentos.len() >= MAX_FRAGMENTOS * 3 {
                break;
            }
        }
    }
    if let Some(scope) = alcance {
        fragmentos.retain(|fragmento| scope.contains(&fragmento.document_id));
    }
    fragmentos.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    fragmentos.truncate(MAX_FRAGMENTOS);
    if fragmentos.is_empty() {
        fragmentos = overview(&mut connection, alcance).await?;
    }
    Ok(fragmentos)
}

async fn search(
    connection: &mut Pooled<'_>,
    term: &str,
    k: usize,
) -> Result<Vec<FragmentoRecuperado>, DbError> {
    let sql = format!(
        "SELECT document_id, pos, page, texto, SCORE() AS score FROM cuy_chunks \
         WHERE SEARCH(texto, {}) USING INVERTED WITH (k = {}, method = 'bm25') LIMIT {}",
        sql_literal(term),
        k,
        k
    );
    let result = match connection.result(&sql).await {
        Ok(result) => result,
        // El motor puede haber perdido el binding del índice: repararlo una vez.
        Err(error) if error.message().contains("no INVERTED index") => {
            match ensure_index(connection).await {
                Ok(()) => match connection.result(&sql).await {
                    Ok(result) => result,
                    Err(_) => return Ok(Vec::new()),
                },
                Err(_) => return Ok(Vec::new()),
            }
        }
        Err(error) => return Err(error),
    };
    let mut fragmentos = Vec::new();
    for row in &result.rows {
        let (Some(document_id), Some(pos), Some(page), Some(texto), Some(score)) = (
            row.first().and_then(as_text),
            row.get(1).and_then(as_int),
            row.get(2).and_then(as_int),
            row.get(3).and_then(as_text),
            row.get(4).and_then(as_float),
        ) else {
            continue;
        };
        fragmentos.push(FragmentoRecuperado {
            document_id,
            pos,
            page: page.max(1) as u32,
            texto,
            score,
        });
    }
    Ok(fragmentos)
}

fn merge(target: &mut Vec<FragmentoRecuperado>, hits: Vec<FragmentoRecuperado>) {
    for hit in hits {
        if let Some(existing) = target
            .iter_mut()
            .find(|fragmento| fragmento.document_id == hit.document_id && fragmento.pos == hit.pos)
        {
            existing.score = existing.score.max(hit.score);
        } else {
            target.push(hit);
        }
    }
}

/// Resumen: primeros fragmentos de hasta 5 documentos del alcance.
async fn overview(
    connection: &mut Pooled<'_>,
    alcance: &Option<HashSet<String>>,
) -> Result<Vec<FragmentoRecuperado>, DbError> {
    let result = connection
        .result(&format!(
            "SELECT document_id, pos, page, texto FROM cuy_chunks ORDER BY pos LIMIT {}",
            OVERVIEW_SCAN_LIMIT
        ))
        .await?;
    let mut per_document: BTreeMap<String, Vec<FragmentoRecuperado>> = BTreeMap::new();
    for row in &result.rows {
        let (Some(document_id), Some(pos), Some(page), Some(texto)) = (
            row.first().and_then(as_text),
            row.get(1).and_then(as_int),
            row.get(2).and_then(as_int),
            row.get(3).and_then(as_text),
        ) else {
            continue;
        };
        if let Some(scope) = alcance {
            if !scope.contains(&document_id) {
                continue;
            }
        }
        per_document
            .entry(document_id.clone())
            .or_default()
            .push(FragmentoRecuperado {
                document_id,
                pos,
                page: page.max(1) as u32,
                texto,
                score: 0.0,
            });
    }
    let mut result = Vec::new();
    for (_, mut chunks) in per_document {
        chunks.sort_by_key(|chunk| chunk.pos);
        result.extend(chunks.into_iter().take(OVERVIEW_CHUNKS_PER_DOC));
        if result.len() >= OVERVIEW_DOCS * OVERVIEW_CHUNKS_PER_DOC {
            break;
        }
    }
    result.truncate(OVERVIEW_DOCS * OVERVIEW_CHUNKS_PER_DOC);
    Ok(result)
}

/// Título de cada documento para construir citas legibles.
pub async fn titulos(db: &Db) -> Result<HashMap<String, String>, DbError> {
    let mut connection = db.acquire().await?;
    let result = connection
        .result("SELECT id, title FROM cuy_documents")
        .await?;
    let mut map = HashMap::new();
    for row in &result.rows {
        if let (Some(id), Some(title)) = (row.first().and_then(as_text), row.get(1).and_then(as_text)) {
            map.insert(id, title);
        }
    }
    Ok(map)
}

/// Construye las citas solo para los marcadores `[n]` que aparecen en la
/// respuesta final del modelo.
pub fn citas(
    fragmentos: &[FragmentoRecuperado],
    titulos: &HashMap<String, String>,
    respuesta: &str,
) -> Vec<Citation> {
    let mut citas = Vec::new();
    for (indice, fragmento) in fragmentos.iter().enumerate() {
        let marcador = format!("[{}]", indice + 1);
        if !respuesta.contains(&marcador) {
            continue;
        }
        citas.push(Citation {
            document_id: fragmento.document_id.clone(),
            document_title: titulos
                .get(&fragmento.document_id)
                .cloned()
                .unwrap_or_else(|| "Documento".to_owned()),
            page: fragmento.page,
            excerpt: recortar(&fragmento.texto, EXCERPT_WORDS),
        });
    }
    citas
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fragmento(id: &str, pos: i32, page: u32) -> FragmentoRecuperado {
        FragmentoRecuperado {
            document_id: id.to_owned(),
            pos,
            page,
            texto: "contenido del fragmento".to_owned(),
            score: 1.0,
        }
    }

    #[test]
    fn solo_cita_los_marcadores_presentes() {
        let fragmentos = vec![fragmento("a", 1, 1), fragmento("b", 1, 2), fragmento("c", 1, 3)];
        let mut titulos = HashMap::new();
        titulos.insert("a".to_owned(), "Paper A".to_owned());
        titulos.insert("b".to_owned(), "Paper B".to_owned());
        let citas = citas(&fragmentos, &titulos, "Según [1] y [3], la conclusión es X.");
        assert_eq!(citas.len(), 2);
        assert_eq!(citas[0].document_id, "a");
        assert_eq!(citas[0].document_title, "Paper A");
        assert_eq!(citas[1].document_id, "c");
        assert_eq!(citas[1].document_title, "Documento");
    }

    #[test]
    fn no_cita_si_no_hay_marcadores() {
        let fragmentos = vec![fragmento("a", 1, 1)];
        let citas = citas(&fragmentos, &HashMap::new(), "No hay fuentes citadas.");
        assert!(citas.is_empty());
    }

    #[test]
    fn extrae_terminos_significativos_sin_stopwords() {
        let terms = significant_terms("¿De qué tratan los documentos que tienes?");
        assert!(terms.contains(&"documentos".to_owned()));
        assert!(!terms.contains(&"que".to_owned()));
        assert!(!terms.contains(&"los".to_owned()));
        assert!(!terms.contains(&"de".to_owned()));
    }

    #[test]
    fn sin_terminos_utiles_devuelve_vacio() {
        assert!(significant_terms("de la el los las un una").is_empty());
        assert!(significant_terms("").is_empty());
    }

    #[test]
    fn fusiona_hits_del_mismo_fragmento_quedandose_con_el_mejor_score() {
        let mut base = vec![fragmento("a", 1, 1)];
        let mut hit = fragmento("a", 1, 1);
        hit.score = 5.0;
        merge(&mut base, vec![hit, fragmento("a", 2, 1)]);
        assert_eq!(base.len(), 2);
        assert_eq!(base[0].score, 5.0);
    }
}
