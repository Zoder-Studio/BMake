/// Removes `// single-line` and `/+ multi-line +\` comments while
/// preserving line structure (so parser line numbers stay accurate).
pub fn strip_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let n = chars.len();
    let mut i = 0;

    while i < n {
        if i + 1 < n && chars[i] == '/' && chars[i + 1] == '+' {
            i += 2;
            while i + 1 < n && !(chars[i] == '+' && chars[i + 1] == '\\') {
                if chars[i] == '\n' {
                    out.push('\n');
                }
                i += 1;
            }
            i = (i + 2).min(n);
            continue;
        }
        if i + 1 < n && chars[i] == '/' && chars[i + 1] == '/' {
            while i < n && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

pub fn to_lines(input: &str) -> Vec<String> {
    strip_comments(input).lines().map(|l| l.to_string()).collect()
}