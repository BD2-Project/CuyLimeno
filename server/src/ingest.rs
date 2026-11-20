//! Extracción de texto de PDF y fragmentación para el índice BM25.

pub const TARGET_WORDS: usize = 300;
pub const MAX_WORDS: usize = 500;
pub const OVERLAP_WORDS: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pagina {
    pub numero: u32,
    pub texto: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fragmento {
    pub pos: u32,
    pub page: u32,
    pub texto: String,
}

/// Extrae el texto separado por páginas. El pipeline es agnóstico al formato:
/// para añadir TXT/DOCX basta con otro parser que devuelva `Vec<Pagina>`.
pub fn extraer_paginas(bytes: &[u8]) -> Result<Vec<Pagina>, String> {
    let paginas = pdf_extract::extract_text_from_mem_by_pages(bytes)
        .map_err(|error| format!("no se pudo extraer texto del PDF: {error}"))?;
    Ok(paginas
        .into_iter()
        .enumerate()
        .map(|(indice, texto)| Pagina {
            numero: (indice + 1) as u32,
            texto: normalizar(&texto),
        })
        .collect())
}

fn normalizar(texto: &str) -> String {
    let collapsed = texto.split_whitespace().collect::<Vec<_>>().join(" ");
    insert_missing_spaces(&collapsed)
}

/// `pdf-extract` a veces pega bloques sin separación ("ResumenLorem",
/// "texto.Resumen"). Inserta espacios donde el límite es evidente.
fn insert_missing_spaces(texto: &str) -> String {
    let chars: Vec<char> = texto.chars().collect();
    let mut result = String::with_capacity(texto.len() + texto.len() / 20);
    for (index, character) in chars.iter().enumerate() {
        if index > 0 {
            let previous = chars[index - 1];
            let next = chars.get(index + 1).copied();
            let boundary = (previous.is_lowercase()
                && character.is_uppercase()
                && next.map(|value| value.is_lowercase()).unwrap_or(true))
                || (matches!(previous, '.' | '!' | '?' | ';' | ':') && character.is_alphabetic())
                || (previous.is_alphabetic() && character.is_numeric());
            if boundary {
                result.push(' ');
            }
        }
        result.push(*character);
    }
    result
}

/// Fragmenta las páginas en ventanas de ~300 palabras (máximo 500, solape 50)
/// intentando cerrar en fin de frase. `pos` es secuencial desde 1.
pub fn fragmentar(paginas: &[Pagina]) -> Vec<Fragmento> {
    let mut fragmentos = Vec::new();
    let mut pos = 1u32;
    for pagina in paginas {
        let palabras: Vec<&str> = pagina.texto.split_whitespace().collect();
        if palabras.is_empty() {
            continue;
        }
        let mut inicio = 0usize;
        while inicio < palabras.len() {
            let mut fin = (inicio + TARGET_WORDS).min(palabras.len());
            if fin < palabras.len() {
                let limite = (inicio + MAX_WORDS).min(palabras.len());
                if let Some(candidato) = (fin..limite).find(|indice| termina_frase(palabras[indice - 1])) {
                    fin = candidato;
                }
            }
            let texto = palabras[inicio..fin].join(" ");
            if !texto.is_empty() {
                fragmentos.push(Fragmento {
                    pos,
                    page: pagina.numero,
                    texto,
                });
                pos += 1;
            }
            if fin >= palabras.len() {
                break;
            }
            inicio = fin.saturating_sub(OVERLAP_WORDS).max(inicio + 1);
        }
    }
    fragmentos
}

fn termina_frase(palabra: &str) -> bool {
    palabra.ends_with('.') || palabra.ends_with('?') || palabra.ends_with('!')
}

/// Recorta a `max_palabras` palabras para los extractos de cita.
pub fn recortar(texto: &str, max_palabras: usize) -> String {
    let palabras: Vec<&str> = texto.split_whitespace().collect();
    if palabras.len() <= max_palabras {
        return texto.to_owned();
    }
    let mut recorte = palabras[..max_palabras].join(" ");
    recorte.push('…');
    recorte
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pagina(numero: u32, texto: &str) -> Pagina {
        Pagina {
            numero,
            texto: normalizar(texto),
        }
    }

    #[test]
    fn pagina_corta_genera_un_fragmento() {
        let paginas = vec![pagina(1, "una introducción breve al tema")];
        let fragmentos = fragmentar(&paginas);
        assert_eq!(fragmentos.len(), 1);
        assert_eq!(fragmentos[0].pos, 1);
        assert_eq!(fragmentos[0].page, 1);
        assert_eq!(fragmentos[0].texto, "una introducción breve al tema");
    }

    #[test]
    fn paginas_vacias_se_omiten_y_pos_es_secuencial() {
        let paginas = vec![
            pagina(1, ""),
            pagina(2, "texto de la segunda página"),
            pagina(3, ""),
            pagina(4, "texto de la cuarta página"),
        ];
        let fragmentos = fragmentar(&paginas);
        assert_eq!(fragmentos.len(), 2);
        assert_eq!(fragmentos[0].pos, 1);
        assert_eq!(fragmentos[0].page, 2);
        assert_eq!(fragmentos[1].pos, 2);
        assert_eq!(fragmentos[1].page, 4);
    }

    #[test]
    fn pagina_larga_se_parte_con_solape_y_respeta_el_maximo() {
        let palabras: Vec<String> = (0..1200).map(|indice| format!("palabra{indice}")).collect();
        let paginas = vec![pagina(1, &palabras.join(" "))];
        let fragmentos = fragmentar(&paginas);
        assert!(fragmentos.len() >= 4);
        for fragmento in &fragmentos {
            let total = fragmento.texto.split_whitespace().count();
            assert!(total <= MAX_WORDS, "fragmento con {total} palabras");
        }
        // El solape hace que el inicio del segundo fragmento aparezca también
        // al final del primero.
        let primero: Vec<&str> = fragmentos[0].texto.split_whitespace().collect();
        let segundo: Vec<&str> = fragmentos[1].texto.split_whitespace().collect();
        assert_eq!(primero.last(), segundo.get(OVERLAP_WORDS - 1));
        assert_eq!(fragmentos[1].pos, 2);
    }

    #[test]
    fn cierra_en_fin_de_frase_cuando_es_posible() {
        let mut texto = String::new();
        for _ in 0..60 {
            texto.push_str("una frase corta. ");
        }
        let paginas = vec![pagina(1, &texto)];
        let fragmentos = fragmentar(&paginas);
        assert!(fragmentos[0].texto.ends_with('.'));
    }

    #[test]
    fn recorta_extractos_largos() {
        let texto = "uno dos tres cuatro cinco seis";
        assert_eq!(recortar(texto, 4), "uno dos tres cuatro…");
        assert_eq!(recortar("uno dos", 4), "uno dos");
    }

    #[test]
    fn separa_bloques_pegados_por_la_extraccion() {
        assert_eq!(
            normalizar("ResumenLorem ipsum dolor"),
            "Resumen Lorem ipsum dolor"
        );
        assert_eq!(normalizar("texto.Resumen del paper"), "texto. Resumen del paper");
        assert_eq!(normalizar("version3 del motor"), "version 3 del motor");
        assert_eq!(normalizar("3.14 y 2.5"), "3.14 y 2.5");
        assert_eq!(normalizar("SoupDB y SQLite"), "SoupDB y SQLite");
        assert_eq!(normalizar("deepLearning"), "deep Learning");
    }
}
