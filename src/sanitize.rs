/// Transliterate Serbian Cyrillic characters to their Serbian Latin equivalents.
///
/// Follows the official Serbian Cyrillic–Latin correspondence (Latinica).
/// Non-Cyrillic characters are passed through unchanged.
///
/// Multi-character outputs (Lj, Nj, Dž) are handled by yielding two/three
/// Latin characters for a single Cyrillic input codepoint, so the output
/// string may be longer than the input.
pub fn cyrillic_to_latin(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            // ── Uppercase ────────────────────────────────────────────────────
            'А' => out.push('A'),
            'Б' => out.push('B'),
            'В' => out.push('V'),
            'Г' => out.push('G'),
            'Д' => out.push('D'),
            'Ђ' => out.push('Đ'),
            'Е' => out.push('E'),
            'Ж' => out.push('Ž'),
            'З' => out.push('Z'),
            'И' => out.push('I'),
            'Ј' => out.push('J'),
            'К' => out.push('K'),
            'Л' => out.push('L'),
            'Љ' => out.push_str("Lj"),
            'М' => out.push('M'),
            'Н' => out.push('N'),
            'Њ' => out.push_str("Nj"),
            'О' => out.push('O'),
            'П' => out.push('P'),
            'Р' => out.push('R'),
            'С' => out.push('S'),
            'Т' => out.push('T'),
            'Ћ' => out.push('Ć'),
            'У' => out.push('U'),
            'Ф' => out.push('F'),
            'Х' => out.push('H'),
            'Ц' => out.push('C'),
            'Ч' => out.push('Č'),
            'Џ' => out.push_str("Dž"),
            'Ш' => out.push('Š'),
            // ── Lowercase ────────────────────────────────────────────────────
            'а' => out.push('a'),
            'б' => out.push('b'),
            'в' => out.push('v'),
            'г' => out.push('g'),
            'д' => out.push('d'),
            'ђ' => out.push('đ'),
            'е' => out.push('e'),
            'ж' => out.push('ž'),
            'з' => out.push('z'),
            'и' => out.push('i'),
            'ј' => out.push('j'),
            'к' => out.push('k'),
            'л' => out.push('l'),
            'љ' => out.push_str("lj"),
            'м' => out.push('m'),
            'н' => out.push('n'),
            'њ' => out.push_str("nj"),
            'о' => out.push('o'),
            'п' => out.push('p'),
            'р' => out.push('r'),
            'с' => out.push('s'),
            'т' => out.push('t'),
            'ћ' => out.push('ć'),
            'у' => out.push('u'),
            'ф' => out.push('f'),
            'х' => out.push('h'),
            'ц' => out.push('c'),
            'ч' => out.push('č'),
            'џ' => out.push_str("dž"),
            'ш' => out.push('š'),
            // ── Pass-through ─────────────────────────────────────────────────
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_transliteration() {
        assert_eq!(cyrillic_to_latin("Хлеб"), "Hleb");
        assert_eq!(cyrillic_to_latin("МЛЕКО"), "MLEKO");
        assert_eq!(cyrillic_to_latin("шећер"), "šećer");
    }

    #[test]
    fn multi_char_outputs() {
        assert_eq!(cyrillic_to_latin("Љубав"), "Ljubav");
        assert_eq!(cyrillic_to_latin("Њујорк"), "Njujork");
        assert_eq!(cyrillic_to_latin("Џем"), "Džem");
    }

    #[test]
    fn mixed_scripts_pass_through() {
        // Already-Latin text should be unchanged.
        assert_eq!(cyrillic_to_latin("ABC 123"), "ABC 123");
        // Mixed input: only Cyrillic chars are replaced.
        assert_eq!(cyrillic_to_latin("Mleko / Млеко"), "Mleko / Mleko");
    }

    #[test]
    fn empty_string() {
        assert_eq!(cyrillic_to_latin(""), "");
    }
}
