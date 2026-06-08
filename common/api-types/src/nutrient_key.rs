pub fn generate(name: &str) -> String {
    let lower = name.to_lowercase();
    let mut result = String::new();
    for c in lower.chars() {
        match c {
            'а' => result.push('a'),
            'б' => result.push('b'),
            'в' => result.push('v'),
            'г' => result.push('g'),
            'д' => result.push('d'),
            'е' | 'ё' => result.push('e'),
            'ж' => result.push_str("zh"),
            'з' => result.push('z'),
            'и' | 'й' => result.push('i'),
            'к' => result.push('k'),
            'л' => result.push('l'),
            'м' => result.push('m'),
            'н' => result.push('n'),
            'о' => result.push('o'),
            'п' => result.push('p'),
            'р' => result.push('r'),
            'с' => result.push('s'),
            'т' => result.push('t'),
            'у' => result.push('u'),
            'ф' => result.push('f'),
            'х' => result.push_str("kh"),
            'ц' => result.push_str("ts"),
            'ч' => result.push_str("ch"),
            'ш' => result.push_str("sh"),
            'щ' => result.push_str("shch"),
            'ъ' | 'ь' => {}
            'ы' => result.push('y'),
            'э' => result.push('e'),
            'ю' => result.push_str("yu"),
            'я' => result.push_str("ya"),
            c if c.is_ascii_alphanumeric() => result.push(c),
            _ => result.push('_'),
        }
    }
    let mut collapsed = String::new();
    for c in result.chars() {
        if c == '_' && collapsed.ends_with('_') {
            continue;
        }
        collapsed.push(c);
    }
    let key = collapsed.trim_matches('_').to_string();
    if key.is_empty() {
        "custom".to_string()
    } else {
        key
    }
}
