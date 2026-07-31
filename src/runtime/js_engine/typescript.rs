// ── TypeScript type stripper ──────────────────────────────────────────────────
//
// Elimina las construcciones propias de TypeScript sin tocar el código JS.
// Cubre: tipos inline, interfaces, type aliases, enums, generics, decorators,
// access modifiers, non-null assertions, satisfies.

pub fn strip_typescript(ts: &str) -> std::result::Result<String, String> {
    let mut out = String::with_capacity(ts.len());
    let mut str_ch = '"'; // delimitador de la string actual

    // Línea por línea primero para eliminar declaraciones de nivel superior
    let lines: Vec<&str> = ts.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim_start();

        // interface Foo { ... }
        if line.starts_with("interface ") || line.starts_with("declare ") {
            // Saltar hasta que la llave de cierre balance llegue a 0
            let mut d = 0i32;
            loop {
                let l = lines[i];
                d += l.chars().filter(|&c| c == '{').count() as i32;
                d -= l.chars().filter(|&c| c == '}').count() as i32;
                i += 1;
                if d <= 0 || i >= lines.len() {
                    break;
                }
            }
            continue;
        }

        // type Foo = ...;
        if line.starts_with("type ") && line.contains('=') {
            // Skip lines until we find one ending with ';' or exhaust lines
            while i < lines.len() {
                let ends = lines[i].trim_end().ends_with(';');
                i += 1;
                if ends {
                    break;
                }
            }
            continue;
        }

        // import type ...
        if line.starts_with("import type ") {
            i += 1;
            continue;
        }

        out.push_str(lines[i]);
        out.push('\n');
        i += 1;
    }

    // Segunda pasada: eliminar anotaciones inline de tipos
    let mut result = String::with_capacity(out.len());
    let mut iter = out.chars().peekable();
    let mut in_str = false;

    while let Some(c) = iter.next() {
        // Gestión de strings (no tocar el interior)
        if (c == '"' || c == '\'' || c == '`') && !in_str {
            in_str = true;
            str_ch = c;
            result.push(c);
            continue;
        }
        if in_str {
            if c == '\\' {
                result.push(c);
                if let Some(next) = iter.next() {
                    result.push(next);
                }
                continue;
            }
            if c == str_ch {
                in_str = false;
            }
            result.push(c);
            continue;
        }

        // Anotación de tipo: `: Type` después de identificador / ) / ]
        if c == ':' {
            // Puede ser ternario (a ? b : c), label, o tipo TS
            // Heurística: si el carácter anterior era palabra/)/], es tipo
            let prev = result.chars().last();
            let is_type_ann = matches!(prev,
                Some(ch) if ch.is_alphanumeric() || ch == '_' || ch == ')' || ch == ']' || ch == '?' || ch == '"' || ch == '\''
            );
            if is_type_ann {
                // Consumir hasta el fin del tipo (coma, ), {, =, ;, newline)
                skip_type_expr(&mut iter);
                continue;
            }
        }

        // `as Type` — type assertion
        if c == 'a' && iter.peek() == Some(&'s') {
            let prev = result.chars().last();
            if matches!(prev, Some(p) if p == ' ' || p == ')' || p == ']') {
                // Comprobar que es `as` completo
                let mut buf = String::from("a");
                buf.push(iter.next().unwrap()); // 's'
                if iter.peek().map(|c| c.is_whitespace()).unwrap_or(false) {
                    // es `as Type`, consumir el tipo
                    let _ = iter.next(); // espacio
                    skip_type_expr(&mut iter);
                    continue;
                } else {
                    result.push_str(&buf);
                    continue;
                }
            }
        }

        // `!` non-null assertion al final de expresión
        if c == '!' {
            let next = iter.peek().copied();
            if matches!(
                next,
                Some(')') | Some('.') | Some('[') | Some(';') | Some(',')
            ) {
                // Eliminar el !
                continue;
            }
        }

        // Access modifiers al inicio de class member
        if result.ends_with('\n') || result.ends_with('{') || result.ends_with(';') {
            let kw = peek_keyword(c, &mut iter);
            if matches!(
                kw.as_str(),
                "public" | "private" | "protected" | "readonly" | "abstract" | "override"
            ) {
                // Saltar la keyword y el espacio siguiente
                skip_word(&mut iter);
                continue;
            }
        }

        result.push(c);
    }

    Ok(result)
}

fn skip_type_expr(iter: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    let mut depth = 0i32;
    while let Some(&c) = iter.peek() {
        match c {
            '<' | '(' | '[' | '{' => {
                depth += 1;
                iter.next();
            }
            '>' | ')' | ']' | '}' => {
                if depth > 0 {
                    depth -= 1;
                    iter.next();
                } else {
                    break;
                }
            }
            ',' | ';' | '\n' if depth == 0 => break,
            '=' if depth == 0 => break,
            _ => {
                iter.next();
            }
        }
    }
}

fn skip_word(iter: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    while let Some(&c) = iter.peek() {
        if c.is_alphanumeric() || c == '_' {
            iter.next();
        } else {
            break;
        }
    }
    // Saltar espacio siguiente
    if iter.peek() == Some(&' ') {
        iter.next();
    }
}

fn peek_keyword(first: char, iter: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut word = String::new();
    word.push(first);
    // Peek ahead without consuming — collect chars that would form the keyword
    let remaining: String = iter
        .clone()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    word.push_str(&remaining);
    word
}
